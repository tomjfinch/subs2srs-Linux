//! Main window: sidebar navigation, settings pages, action bar.

use crate::job;
use crate::ui::{self, AppState, Lens, Pick};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use subs2srs_core::encoding::ENCODINGS;
use subs2srs_core::ffmpeg;
use subs2srs_core::files::{self, AUDIO_EXTS, VIDEO_EXTS};
use subs2srs_core::model::Episode;
use subs2srs_core::pipeline;
use subs2srs_core::progress::Progress;
use subs2srs_core::settings::{self, AudioFormat, AudioSource, Prefs, Project, ShiftRule, Theme, VideoCodec, Which};
use subs2srs_core::subs;
use subs2srs_core::Error;

const SUB_FILTER: (&str, &[&str]) = ("Subtitles and videos", &["ass", "ssa", "srt", "vtt", "lrc", "mkv", "mp4", "webm", "m4v", "mks"]);
const VIDEO_FILTER: (&str, &[&str]) = ("Videos", VIDEO_EXTS);
const AUDIO_FILTER: (&str, &[&str]) = ("Audio", AUDIO_EXTS);
const PROJECT_EXT: &str = "s2s.json";

pub struct Ctx {
    pub st: Rc<AppState>,
    pub win: gtk::ApplicationWindow,
    status: gtk::Label,
    bar: gtk::ProgressBar,
    go: gtk::Button,
    preview_btn: gtk::Button,
    running: RefCell<Option<Arc<Progress>>>,
    title: gtk::Label,
    subtitle: gtk::Label,
    force_close: Cell<bool>,
}

pub struct MainWindow;

impl MainWindow {
    pub fn new(app: &gtk::Application, open: Option<PathBuf>) -> gtk::ApplicationWindow {
        let prefs = Prefs::load();
        crate::theme::set_mode(prefs.theme);
        let st = AppState::new(settings::load_default_project(), prefs.clone());

        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("subs2srs")
            .default_width(prefs.window_width.max(760))
            .default_height(prefs.window_height.max(520))
            .build();

        // ── Header bar ────────────────────────────────────────────────
        let header = gtk::HeaderBar::new();
        let title_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).valign(gtk::Align::Center).build();
        let title = gtk::Label::builder().label("subs2srs").css_classes(["title"]).build();
        let subtitle = gtk::Label::builder().css_classes(["subtitle"]).visible(false).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        title_box.append(&title);
        title_box.append(&subtitle);
        header.set_title_widget(Some(&title_box));

        let open_btn = gtk::Button::builder().icon_name("document-open-symbolic").tooltip_text("Open project (Ctrl+O)").action_name("win.open").build();
        let save_btn = gtk::Button::builder().icon_name("document-save-symbolic").tooltip_text("Save project (Ctrl+S)").action_name("win.save").build();
        header.pack_start(&open_btn);
        header.pack_start(&save_btn);

        let menu = gio::Menu::new();
        let s1 = gio::Menu::new();
        s1.append(Some("New Project"), Some("win.new"));
        s1.append(Some("Open Project…"), Some("win.open"));
        s1.append(Some("Save Project"), Some("win.save"));
        s1.append(Some("Save Project As…"), Some("win.save-as"));
        menu.append_section(None, &s1);
        let s2 = gio::Menu::new();
        s2.append(Some("Use Current Settings as Default"), Some("win.save-defaults"));
        menu.append_section(None, &s2);
        let theme_menu = gio::Menu::new();
        theme_menu.append(Some("Follow System"), Some("win.theme::system"));
        theme_menu.append(Some("Light"), Some("win.theme::light"));
        theme_menu.append(Some("Dark"), Some("win.theme::dark"));
        let s3 = gio::Menu::new();
        s3.append_submenu(Some("Appearance"), &theme_menu);
        s3.append(Some("Preferences"), Some("win.preferences"));
        s3.append(Some("About subs2srs"), Some("win.about"));
        menu.append_section(None, &s3);
        let menu_btn = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).tooltip_text("Main menu").primary(true).build();
        header.pack_end(&menu_btn);
        win.set_titlebar(Some(&header));

        // ── Action bar ───────────────────────────────────────────────
        let status = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["s2s-status"]).build();
        let bar = gtk::ProgressBar::builder().hexpand(true).visible(false).valign(gtk::Align::Center).build();
        let preview_btn = gtk::Button::builder().label("Preview").tooltip_text("Check and edit the matched lines (Ctrl+P)").action_name("win.preview").build();
        let go = gtk::Button::builder().label("Generate").css_classes(["suggested-action"]).tooltip_text("Create the Anki deck (Ctrl+Enter)").build();
        go.set_width_request(120);

        let ctx = Rc::new(Ctx {
            st: st.clone(),
            win: win.clone(),
            status: status.clone(),
            bar: bar.clone(),
            go: go.clone(),
            preview_btn: preview_btn.clone(),
            running: RefCell::new(None),
            title,
            subtitle,
            force_close: Cell::new(false),
        });

        // ── Pages ─────────────────────────────────────────────────────
        let stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).hexpand(true).vexpand(true).build();
        let sidebar = gtk::ListBox::builder().css_classes(["navigation-sidebar", "s2s-sidebar"]).build();
        let pages: Vec<(&str, &str, &str, gtk::Widget)> = vec![
            ("source", "Source", "s2s-source-symbolic", page_source(&ctx).upcast()),
            ("timing", "Timing", "s2s-timing-symbolic", page_timing(&ctx).upcast()),
            ("audio", "Audio", "s2s-audio-symbolic", page_audio(&ctx).upcast()),
            ("snapshots", "Snapshots", "s2s-snapshot-symbolic", page_snapshots(&ctx).upcast()),
            ("video", "Video Clips", "s2s-video-symbolic", page_video(&ctx).upcast()),
            ("filters", "Filters", "s2s-filter-symbolic", page_filters(&ctx).upcast()),
            ("context", "Context", "s2s-context-symbolic", page_context(&ctx).upcast()),
            ("tools", "Tools", "s2s-tools-symbolic", page_tools(&ctx).upcast()),
        ];
        for (name, label, icon, widget) in pages {
            stack.add_named(&widget, Some(name));
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            b.append(&gtk::Image::from_icon_name(icon));
            b.append(&gtk::Label::builder().label(label).xalign(0.0).build());
            let row = gtk::ListBoxRow::builder().child(&b).build();
            row.set_widget_name(name);
            sidebar.append(&row);
        }
        let stk = stack.clone();
        sidebar.connect_row_selected(move |_, row| {
            if let Some(r) = row {
                stk.set_visible_child_name(&r.widget_name());
            }
        });
        let start = std::env::var("SUBS2SRS_PAGE").ok().and_then(|p| p.parse::<i32>().ok()).unwrap_or(0);
        sidebar.select_row(sidebar.row_at_index(start).as_ref());

        let side_pane = gtk::Box::builder().orientation(gtk::Orientation::Vertical).width_request(196).css_classes(["s2s-sidebar-pane"]).build();
        side_pane.append(&gtk::Box::builder().height_request(8).build());
        side_pane.append(&sidebar);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&side_pane);
        body.append(&stack);

        let actionbar = gtk::Box::builder().spacing(10).css_classes(["s2s-actionbar"]).build();
        let status_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).hexpand(true).valign(gtk::Align::Center).build();
        status_box.append(&status);
        status_box.append(&bar);
        actionbar.append(&status_box);
        actionbar.append(&preview_btn);
        actionbar.append(&go);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&body);
        root.append(&actionbar);
        win.set_child(Some(&root));

        install_actions(&ctx);
        let c = ctx.clone();
        go.connect_clicked(move |_| on_go(&c, None));

        // Live summary in the status line.
        let c = ctx.clone();
        st.on_change(move || {
            update_title(&c);
            if c.running.borrow().is_none() {
                c.status.set_text(&summary_line(&c.st.project.borrow()));
            }
        });

        let c = ctx.clone();
        win.connect_close_request(move |w| {
            if c.force_close.get() || !c.st.dirty.get() {
                save_window_prefs(&c);
                return glib::Propagation::Proceed;
            }
            let c2 = c.clone();
            let w2 = w.clone();
            ui::confirm(
                w,
                "Save changes to this project?",
                "Your changes will be lost if you don't save them.",
                &["Cancel", "Discard", "Save"],
                move |i| match i {
                    1 => {
                        c2.force_close.set(true);
                        w2.close();
                    }
                    2 => save_project(&c2, false, Some(Box::new(move |c3: &Rc<Ctx>| {
                        c3.force_close.set(true);
                        c3.win.close();
                    }))),
                    _ => {}
                },
            );
            glib::Propagation::Stop
        });

        if let Some(path) = open {
            load_project_file(&ctx, &path);
        }
        update_title(&ctx);
        // Development aid: SUBS2SRS_DEBUG_ACTION=win.preview opens a window on start.
        if let Ok(action) = std::env::var("SUBS2SRS_DEBUG_ACTION") {
            let w = win.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                for a in action.split(',') {
                    let _ = WidgetExt::activate_action(&w, a.trim(), None);
                }
            });
        }
        win
    }
}

fn save_window_prefs(c: &Rc<Ctx>) {
    let mut p = c.st.prefs.borrow_mut();
    let (w, h) = c.win.default_size();
    if !c.win.is_maximized() && w > 0 && h > 0 {
        p.window_width = w;
        p.window_height = h;
    }
    // Other windows may have changed prefs; keep their theme/format edits.
    let disk = Prefs::load();
    let mut merged = disk;
    merged.window_width = p.window_width;
    merged.window_height = p.window_height;
    merged.last_dir = p.last_dir.clone();
    let _ = merged.save();
}

fn update_title(c: &Ctx) {
    let path = c.st.path.borrow().clone();
    let dirty = if c.st.dirty.get() { " •" } else { "" };
    match path {
        Some(p) => {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            c.title.set_text(&format!("{name}{dirty}"));
            c.subtitle.set_text(&p.parent().map(|d| d.display().to_string()).unwrap_or_default());
            c.subtitle.set_visible(true);
            c.win.set_title(Some(&format!("{name} — subs2srs")));
        }
        None => {
            let deck = c.st.project.borrow().deck_name.trim().to_string();
            c.title.set_text(&format!("{}{dirty}", if deck.is_empty() { "subs2srs" } else { &deck }));
            c.subtitle.set_text("Unsaved project");
            c.subtitle.set_visible(!deck.is_empty() || c.st.dirty.get());
            c.win.set_title(Some("subs2srs"));
        }
    }
}

fn summary_line(p: &Project) -> String {
    let mut out = Vec::new();
    let mut media = Vec::new();
    if p.audio.enabled {
        media.push("audio");
    }
    if p.snapshots.enabled {
        media.push("snapshots");
    }
    if p.video_clips.enabled {
        media.push("video clips");
    }
    let deck = p.clean_deck_name();
    if deck.is_empty() {
        out.push("Set the subtitles, video, deck name and output folder, then press Generate.".to_string());
    } else {
        out.push(format!("Deck “{deck}”"));
        out.push(if media.is_empty() { "text only".into() } else { media.join(", ") });
        if p.has_subs2() {
            out.push("bilingual".into());
        }
    }
    out.join(" · ")
}

// ── Actions ─────────────────────────────────────────────────────────────

fn add_action(c: &Rc<Ctx>, name: &str, f: impl Fn(&Rc<Ctx>) + 'static) {
    let a = gio::SimpleAction::new(name, None);
    let c2 = c.clone();
    a.connect_activate(move |_, _| f(&c2));
    c.win.add_action(&a);
}

fn install_actions(c: &Rc<Ctx>) {
    add_action(c, "new", |c| {
        let c2 = c.clone();
        let go = move || c2.st.load_project(settings::load_default_project(), None);
        if c.st.dirty.get() {
            ui::confirm(&c.win, "Start a new project?", "Unsaved changes to the current project will be lost.", &["Cancel", "New Project"], move |i| {
                if i == 1 {
                    go()
                }
            });
        } else {
            go();
        }
    });
    add_action(c, "open", open_project);
    add_action(c, "save", |c| save_project(c, false, None));
    add_action(c, "save-as", |c| save_project(c, true, None));
    add_action(c, "save-defaults", |c| {
        let mut p = c.st.snapshot();
        // Inputs are per show; defaults only keep the options.
        p.subs1.pattern.clear();
        p.subs2.pattern.clear();
        p.video_pattern.clear();
        p.audio.pattern.clear();
        p.deck_name.clear();
        p.subs1.shift_rules.clear();
        p.subs2.shift_rules.clear();
        p.actors.clear();
        match p.save(&settings::default_project_path()) {
            Ok(()) => c.status.set_text("Saved as the default for new projects."),
            Err(e) => ui::alert(&c.win, "Could not save defaults", &e.to_string()),
        }
    });
    add_action(c, "preview", |c| {
        if c.running.borrow().is_some() {
            return;
        }
        let c2 = c.clone();
        crate::preview::open(&c.win, c.st.clone(), Rc::new(move |eps| on_go(&c2, Some(eps))));
    });
    add_action(c, "generate", |c| on_go(c, None));
    add_action(c, "dueling", |c| crate::tools_window::dueling(&c.win, c.st.clone()));
    add_action(c, "extract-audio", |c| crate::tools_window::extract_audio(&c.win, c.st.clone()));
    add_action(c, "preferences", |c| crate::prefs_window::open(&c.win, c.st.clone()));
    add_action(c, "close", |c| c.win.close());
    add_action(c, "about", |c| {
        let d = gtk::AboutDialog::builder()
            .transient_for(&c.win)
            .modal(true)
            .program_name("subs2srs")
            .logo_icon_name(crate::APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .comments("Create Anki flashcards from videos and subtitles for language learning.\nRust/GTK4 port.")
            .license_type(gtk::License::Gpl30)
            .website("https://github.com/Ajatt-Tools/subs2srs")
            .website_label("Original project")
            .authors(["Christopher Brochtrup (original subs2srs)", "erjiang (Linux/Mono port)", "fkzys and contributors (GTK4/.NET port)", "Rust/GTK4 port contributors"])
            .build();
        d.present();
    });

    let theme = c.st.prefs.borrow().theme;
    let state = theme_name(theme).to_variant();
    let a = gio::SimpleAction::new_stateful("theme", Some(glib::VariantTy::STRING), &state);
    let c2 = c.clone();
    a.connect_change_state(move |a, v| {
        let Some(v) = v else { return };
        let mode = match v.str().unwrap_or("system") {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        };
        a.set_state(v);
        crate::theme::set_mode(mode);
        c2.st.prefs.borrow_mut().theme = mode;
        let mut disk = Prefs::load();
        disk.theme = mode;
        let _ = disk.save();
    });
    c.win.add_action(&a);
}

pub fn theme_name(t: Theme) -> &'static str {
    match t {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

fn project_filter() -> gio::ListStore {
    let f = gtk::FileFilter::new();
    f.set_name(Some("subs2srs projects"));
    f.add_suffix(PROJECT_EXT);
    f.add_suffix("json");
    let store = gio::ListStore::new::<gtk::FileFilter>();
    store.append(&f);
    store
}

fn open_project(c: &Rc<Ctx>) {
    let d = gtk::FileDialog::builder().title("Open Project").modal(true).filters(&project_filter()).build();
    let c2 = c.clone();
    d.open(Some(&c.win), gio::Cancellable::NONE, move |r| {
        if let Ok(f) = r {
            if let Some(p) = f.path() {
                load_project_file(&c2, &p);
            }
        }
    });
}

fn load_project_file(c: &Rc<Ctx>, path: &std::path::Path) {
    match Project::load(path) {
        Ok(p) => {
            c.st.load_project(p, Some(path.to_path_buf()));
            update_title(c);
        }
        Err(e) => ui::alert(&c.win, "Could not open project", &format!("{}\n\n{e}", path.display())),
    }
}

type AfterSave = Box<dyn FnOnce(&Rc<Ctx>)>;

fn save_project(c: &Rc<Ctx>, ask: bool, after: Option<AfterSave>) {
    let path = c.st.path.borrow().clone();
    let write = move |c: &Rc<Ctx>, path: PathBuf, after: Option<AfterSave>| {
        let path = if path.to_string_lossy().ends_with(".json") { path } else { PathBuf::from(format!("{}.{PROJECT_EXT}", path.display())) };
        match c.st.snapshot().save(&path) {
            Ok(()) => {
                *c.st.path.borrow_mut() = Some(path);
                c.st.dirty.set(false);
                update_title(c);
                c.status.set_text("Project saved.");
                if let Some(f) = after {
                    f(c);
                }
            }
            Err(e) => ui::alert(&c.win, "Could not save project", &e.to_string()),
        }
    };
    match path {
        Some(p) if !ask => write(c, p, after),
        _ => {
            let deck = c.st.project.borrow().clean_deck_name();
            let d = gtk::FileDialog::builder()
                .title("Save Project")
                .modal(true)
                .initial_name(format!("{}.{PROJECT_EXT}", if deck.is_empty() { "project" } else { &deck }))
                .filters(&project_filter())
                .build();
            let out = c.st.project.borrow().output_dir.clone();
            if !out.is_empty() && PathBuf::from(&out).is_dir() {
                d.set_initial_folder(Some(&gio::File::for_path(&out)));
            }
            let c2 = c.clone();
            d.save(Some(&c.win), gio::Cancellable::NONE, move |r| {
                if let Ok(f) = r {
                    if let Some(p) = f.path() {
                        write(&c2, p, after);
                    }
                }
            });
        }
    }
}

// ── Generate ────────────────────────────────────────────────────────────

fn set_running(c: &Rc<Ctx>, running: Option<Arc<Progress>>) {
    let busy = running.is_some();
    *c.running.borrow_mut() = running;
    c.bar.set_visible(busy);
    c.preview_btn.set_sensitive(!busy);
    if busy {
        c.go.set_label("Cancel");
        c.go.remove_css_class("suggested-action");
        c.go.add_css_class("destructive-action");
    } else {
        c.go.set_label("Generate");
        c.go.remove_css_class("destructive-action");
        c.go.add_css_class("suggested-action");
    }
}

pub fn on_go(c: &Rc<Ctx>, episodes: Option<Vec<Episode>>) {
    if let Some(p) = c.running.borrow().as_ref() {
        p.cancel();
        c.status.set_text("Cancelling…");
        return;
    }
    let project = c.st.snapshot();
    let prefs = c.st.prefs.borrow().clone();
    let progress = Arc::new(Progress::new());
    set_running(c, Some(progress.clone()));
    c.status.set_text("Starting…");
    let (c_tick, c_done) = (c.clone(), c.clone());
    job::run(
        progress,
        move |p| pipeline::generate(&project, &prefs, episodes, p),
        move |s, overall| {
            if c_tick.running.borrow().as_ref().map(|p| p.is_cancelled()).unwrap_or(false) {
                return;
            }
            let step = if s.steps > 0 { format!("Step {}/{} · ", s.step, s.steps) } else { String::new() };
            let detail = if s.detail.is_empty() { String::new() } else { format!(" — {}", s.detail) };
            c_tick.status.set_text(&format!("{step}{}{detail}", s.step_name));
            match overall {
                Some(f) => c_tick.bar.set_fraction(f),
                None => c_tick.bar.pulse(),
            }
        },
        move |r| {
            set_running(&c_done, None);
            match r {
                Ok(summary) => {
                    c_done.status.set_text(&format!("Done: {} cards in {:.1} s.", summary.cards, summary.elapsed_secs));
                    let tsv = summary.tsv_path.clone();
                    let win = c_done.win.clone();
                    ui::confirm(&c_done.win, "Deck created", &summary.message(), &["Close", "Open Folder"], move |i| {
                        if i == 1 {
                            let l = gtk::FileLauncher::new(Some(&gio::File::for_path(&tsv)));
                            l.open_containing_folder(Some(&win), gio::Cancellable::NONE, |_| {});
                        }
                    });
                }
                Err(Error::Cancelled) => c_done.status.set_text("Cancelled. Files created so far are kept and will be reused next time."),
                Err(e) => {
                    c_done.status.set_text("Failed.");
                    ui::alert(&c_done.win, "Could not create the deck", &e.to_string());
                }
            }
        },
    );
}

// ── Helpers for pages ───────────────────────────────────────────────────

fn encoding_dropdown(st: &Rc<AppState>, lens: Lens<String>) -> gtk::DropDown {
    let labels: Vec<&str> = ENCODINGS.iter().map(|(_, n)| *n).collect();
    let dd = gtk::DropDown::from_strings(&labels);
    dd.set_valign(gtk::Align::Center);
    dd.set_tooltip_text(Some("Text encoding (ignored for embedded tracks)"));
    dd.set_selected(subs2srs_core::encoding::index_of(&st.get(lens)) as u32);
    let s = st.clone();
    dd.connect_selected_notify(move |dd| {
        if let Some((l, _)) = ENCODINGS.get(dd.selected() as usize) {
            s.set(lens, l.to_string());
        }
    });
    let (s, w) = (st.clone(), dd.downgrade());
    st.on_load(move || {
        if let Some(dd) = w.upgrade() {
            dd.set_selected(subs2srs_core::encoding::index_of(&s.get(lens)) as u32);
        }
    });
    dd
}

/// Dropdown whose entries come from probing a media file. Selection is
/// stored as an index in the project.
struct StreamChoice {
    dd: gtk::DropDown,
    model: gtk::StringList,
    updating: Cell<bool>,
    lens: Lens<u32>,
    st: Rc<AppState>,
}

impl StreamChoice {
    fn new(st: &Rc<AppState>, lens: Lens<u32>) -> Rc<Self> {
        let model = gtk::StringList::new(&[]);
        let dd = gtk::DropDown::new(Some(model.clone()), None::<gtk::Expression>);
        dd.set_valign(gtk::Align::Center);
        let sc = Rc::new(StreamChoice { dd, model, updating: Cell::new(false), lens, st: st.clone() });
        let w = Rc::downgrade(&sc);
        sc.dd.connect_selected_notify(move |dd| {
            if let Some(sc) = w.upgrade() {
                if !sc.updating.get() && dd.selected() != gtk::INVALID_LIST_POSITION {
                    sc.st.set(sc.lens, dd.selected());
                }
            }
        });
        let w = Rc::downgrade(&sc);
        st.on_load(move || {
            if let Some(sc) = w.upgrade() {
                sc.select();
            }
        });
        sc
    }
    fn fill(&self, items: &[String]) {
        self.updating.set(true);
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        self.updating.set(false);
        self.select();
    }
    fn select(&self) {
        let want = self.st.get(self.lens);
        self.updating.set(true);
        if want < self.model.n_items() {
            self.dd.set_selected(want);
        } else if self.model.n_items() > 0 {
            // Keep the stored index even if this file has fewer streams.
            self.dd.set_selected(0);
        }
        self.updating.set(false);
    }
}

/// Watch a pattern; when its first file changes, probe it in the background.
fn watch_first_file(st: &Rc<AppState>, lens: Lens<String>, exts: &'static [&'static str], on_file: impl Fn(Option<PathBuf>, usize) + 'static) {
    let last: RefCell<Option<(String, Option<PathBuf>, usize)>> = RefCell::new(None);
    let s = st.clone();
    st.on_change(move || {
        let pat = s.get(lens);
        if last.borrow().as_ref().map(|(p, _, _)| *p == pat).unwrap_or(false) {
            return;
        }
        let found = files::expand(&pat, exts);
        let first = found.first().cloned();
        let n = found.len();
        *last.borrow_mut() = Some((pat, first.clone(), n));
        on_file(first, n);
    });
}

fn count_badge(badge: &gtk::Label, pattern: &str, n: usize, what: &str, expect: Option<usize>) {
    if pattern.trim().is_empty() {
        badge.set_visible(false);
        return;
    }
    let s = if n == 1 { "" } else { "s" };
    match (n, expect) {
        (0, _) => ui::set_badge(badge, "No files match", "error"),
        (n, Some(e)) if e > 0 && n != e => ui::set_badge(badge, &format!("{n} {what}{s} · Subs1 has {e}"), "warn"),
        (n, _) => ui::set_badge(badge, &format!("{n} {what}{s}"), "ok"),
    }
}

fn sensitive_when(st: &Rc<AppState>, widgets: Vec<gtk::Widget>, cond: fn(&Project) -> bool) {
    let s = st.clone();
    st.on_change(move || {
        let on = cond(&s.project.borrow());
        for w in &widgets {
            w.set_sensitive(on);
        }
    });
}

fn visible_when(st: &Rc<AppState>, w: gtk::Widget, cond: fn(&Project) -> bool) {
    let s = st.clone();
    st.on_change(move || w.set_visible(cond(&s.project.borrow())));
}

fn sub_exts() -> &'static [&'static str] {
    static EXTS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    EXTS.get_or_init(pipeline::subtitle_exts)
}

fn subs1_count(st: &AppState) -> usize {
    files::expand(&st.project.borrow().subs1.pattern, sub_exts()).len()
}

// ── Source page ─────────────────────────────────────────────────────────

fn subs_rows(c: &Rc<Ctx>, list: &gtk::ListBox, which: Which) {
    let st = &c.st;
    let (title, sub, pat, enc, track): (&str, &str, Lens<String>, Lens<String>, Lens<u32>) = match which {
        Which::Subs1 => (
            "Subs1 · target language",
            "The language you're learning. ASS/SSA, SRT, WebVTT or LRC files — or videos with embedded subtitles.",
            |p| &mut p.subs1.pattern,
            |p| &mut p.subs1.encoding,
            |p| &mut p.subs1.embedded_track,
        ),
        Which::Subs2 => (
            "Subs2 · your language (optional)",
            "Translations shown on the back of the card.",
            |p| &mut p.subs2.pattern,
            |p| &mut p.subs2.encoding,
            |p| &mut p.subs2.embedded_track,
        ),
    };
    let (_, body, badge) = ui::wide_row(list, title, sub);
    let (picker, _) = ui::path_picker(st, &c.win, pat, Pick::Files, "Choose Subtitles", Some(SUB_FILTER));
    body.append(&picker);
    body.append(&encoding_dropdown(st, enc));

    let track_choice = StreamChoice::new(st, track);
    let track_row = ui::row_with(list, "Embedded subtitle track", "Text subtitles inside the video are extracted with ffmpeg.", &track_choice.dd);
    track_row.row.set_visible(false);
    let tr = track_row.row.clone();
    let tc = track_choice.clone();
    let s = st.clone();
    let badge2 = badge.clone();
    watch_first_file(st, pat, sub_exts(), move |first, n| {
        let pattern = s.get(pat);
        let expect = if which == Which::Subs2 { Some(subs1_count(&s)) } else { None };
        count_badge(&badge2, &pattern, n, "file", expect);
        let container = first.as_ref().map(|f| subs::is_container(f)).unwrap_or(false);
        tr.set_visible(container);
        if let (true, Some(f)) = (container, first) {
            tc.fill(&["Reading tracks…".to_string()]);
            let tc2 = tc.clone();
            job::background(move || ffmpeg::probe(&f), move |r| match r {
                Ok(info) => {
                    let items: Vec<String> = info
                        .subtitles
                        .iter()
                        .map(|s| {
                            let d = s.describe();
                            if s.is_text_subtitle() { d } else { format!("{d} — bitmap, not supported") }
                        })
                        .collect();
                    tc2.fill(&if items.is_empty() { vec!["No subtitle tracks".into()] } else { items });
                }
                Err(e) => tc2.fill(&[format!("Can't read file: {e}")]),
            });
        }
    });
}

fn page_source(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Source", "Pick the subtitles and videos for one or more episodes. Files are matched in natural order (Episode 2 before Episode 10). You can also drag files onto the fields.");

    let g = ui::group(&b, "Subtitles", None);
    subs_rows(c, &g, Which::Subs1);
    subs_rows(c, &g, Which::Subs2);

    let g = ui::group(&b, "Video", Some("Needed for snapshots, video clips and audio taken from the video."));
    let (_, body, badge) = ui::wide_row(&g, "Video files", "");
    let (picker, _) = ui::path_picker(st, &c.win, |p| &mut p.video_pattern, Pick::Files, "Choose Videos", Some(VIDEO_FILTER));
    body.append(&picker);
    let audio_choice = StreamChoice::new(st, |p| &mut p.audio_stream);
    let ar = ui::row_with(&g, "Audio track", "Used for audio clips and video clips.", &audio_choice.dd);
    let ac = audio_choice.clone();
    let s = st.clone();
    let ar_sub = ar.subtitle.clone();
    watch_first_file(st, |p| &mut p.video_pattern, VIDEO_EXTS, move |first, n| {
        count_badge(&badge, &s.get(|p| &mut p.video_pattern), n, "video", Some(subs1_count(&s)));
        match first {
            None => ac.fill(&[]),
            Some(f) => {
                ac.fill(&["Reading tracks…".to_string()]);
                let ac2 = ac.clone();
                let sub = ar_sub.clone();
                job::background(move || ffmpeg::probe(&f), move |r| match r {
                    Ok(info) => {
                        let items: Vec<String> = info.audio.iter().map(|a| a.describe()).collect();
                        ac2.fill(&if items.is_empty() { vec!["No audio".into()] } else { items });
                        let dur = subs2srs_core::time::to_hms(info.duration);
                        let res = info.video.first().map(|v| format!(" · {}×{}", v.width, v.height)).unwrap_or_default();
                        ui::set_subtitle(&sub, &format!("Used for audio clips and video clips. First episode: {dur}{res}"));
                    }
                    Err(e) => {
                        ac2.fill(&["Unreadable".into()]);
                        ui::set_subtitle(&sub, &e.to_string());
                    }
                });
            }
        }
    });

    let g = ui::group(&b, "Deck", None);
    let deck = ui::entry(st, |p| &mut p.deck_name);
    deck.set_placeholder_text(Some("e.g. Shirokuma_Cafe"));
    deck.set_width_chars(24);
    deck.set_hexpand(false);
    ui::row_with(&g, "Deck name", "Used in file names and as the Anki tag. Spaces become underscores.", &deck);
    let (_, body, _) = ui::wide_row(&g, "Output folder", "The .tsv import file and a <deck>.media folder are created here.");
    body.append(&ui::path_picker(st, &c.win, |p| &mut p.output_dir, Pick::Folder, "Choose Output Folder", None).0);
    ui::row_with(&g, "First episode number", "", &ui::spin(st, |p| &mut p.first_episode, 0.0, 99999.0, 1.0));
    ui::row_with(&g, "Episodes to process", "0 processes every episode.", &ui::spin(st, |p| &mut p.episode_limit, 0.0, 99999.0, 1.0));
    pg
}

// ── Timing page ─────────────────────────────────────────────────────────

fn shift_rules_row(c: &Rc<Ctx>, list: &gtk::ListBox, which: Which) -> gtk::ListBoxRow {
    let st = c.st.clone();
    let title = match which {
        Which::Subs1 => "Subs1 per-episode shifts",
        Which::Subs2 => "Subs2 per-episode shifts",
    };
    let (row, body, _) = ui::wide_row(list, title, "From the given episode on, use a different shift (cascading).");
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 6);
    vb.set_hexpand(true);
    body.append(&vb);
    let rules_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    vb.append(&rules_box);
    let add = gtk::Button::builder().child(&ui::icon_label("list-add-symbolic", "Add rule")).halign(gtk::Align::Start).css_classes(["flat"]).build();
    vb.append(&add);

    fn rules_mut(p: &mut Project, w: Which) -> &mut Vec<ShiftRule> {
        match w {
            Which::Subs1 => &mut p.subs1.shift_rules,
            Which::Subs2 => &mut p.subs2.shift_rules,
        }
    }

    let rebuild: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let (rb, s, r2) = (rules_box.clone(), st.clone(), rebuild.clone());
    let f: Rc<dyn Fn()> = Rc::new(move || {
        while let Some(ch) = rb.first_child() {
            rb.remove(&ch);
        }
        let rules = rules_mut(&mut s.project.borrow_mut(), which).clone();
        for (i, r) in rules.iter().enumerate() {
            let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            hb.append(&gtk::Label::new(Some("From episode")));
            let ep = gtk::SpinButton::with_range(0.0, 99999.0, 1.0);
            ep.set_value(r.from_episode as f64);
            hb.append(&ep);
            hb.append(&gtk::Label::new(Some("shift")));
            let ms = gtk::SpinButton::with_range(-3_600_000.0, 3_600_000.0, 10.0);
            ms.set_value(r.shift_ms as f64);
            hb.append(&ms);
            hb.append(&gtk::Label::new(Some("ms")));
            let del = gtk::Button::builder().icon_name("user-trash-symbolic").css_classes(["flat"]).tooltip_text("Remove rule").build();
            let sp = gtk::Box::builder().hexpand(true).build();
            hb.append(&sp);
            hb.append(&del);
            let s2 = s.clone();
            ep.connect_value_changed(move |sb| {
                let v = sb.value() as i64;
                s2.update(|p| {
                    if let Some(r) = rules_mut(p, which).get_mut(i) {
                        r.from_episode = v
                    }
                })
            });
            let s2 = s.clone();
            ms.connect_value_changed(move |sb| {
                let v = sb.value() as i64;
                s2.update(|p| {
                    if let Some(r) = rules_mut(p, which).get_mut(i) {
                        r.shift_ms = v
                    }
                })
            });
            let (s2, r3) = (s.clone(), r2.clone());
            del.connect_clicked(move |_| {
                s2.update(|p| {
                    let v = rules_mut(p, which);
                    if i < v.len() {
                        v.remove(i);
                    }
                });
                let f = r3.borrow().clone();
                if let Some(f) = f {
                    glib::idle_add_local_once(move || f());
                }
            });
            rb.append(&hb);
        }
    });
    *rebuild.borrow_mut() = Some(f.clone());
    f();
    let (s, f2) = (st.clone(), f.clone());
    add.connect_clicked(move |_| {
        s.update(|p| {
            let v = rules_mut(p, which);
            let next = v.iter().map(|r| r.from_episode).max().unwrap_or(p.first_episode) + 1;
            let shift = match which {
                Which::Subs1 => p.subs1.time_shift_ms,
                Which::Subs2 => p.subs2.time_shift_ms,
            };
            rules_mut(p, which).push(ShiftRule { from_episode: next, shift_ms: shift });
        });
        f2();
    });
    st.on_load(move || f());
    row
}

fn page_timing(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Timing", "Which subtitle decides when a card starts and ends, and how to fix subtitles that are out of sync.");
    let g = ui::group(&b, "Card timing", None);
    ui::row_with(
        &g,
        "Use timings from",
        "Pick the subtitle whose timing matches the audio best.",
        &ui::dropdown(st, |p| &mut p.timings_from, vec![(Which::Subs1, "Subs1".into()), (Which::Subs2, "Subs2".into())]),
    );

    let g = ui::group(&b, "Time shift", Some("Positive values make the subtitles appear later, negative earlier."));
    ui::row_with(&g, "Shift subtitles", "", &ui::switch(st, |p| &mut p.time_shift_enabled));
    let r1 = ui::row_with(&g, "Subs1 shift", "Milliseconds", &ui::spin(st, |p| &mut p.subs1.time_shift_ms, -3_600_000.0, 3_600_000.0, 10.0));
    let r2 = ui::row_with(&g, "Subs2 shift", "Milliseconds", &ui::spin(st, |p| &mut p.subs2.time_shift_ms, -3_600_000.0, 3_600_000.0, 10.0));
    let r3 = shift_rules_row(c, &g, Which::Subs1);
    let r4 = shift_rules_row(c, &g, Which::Subs2);
    sensitive_when(st, vec![r1.row.upcast(), r2.row.upcast(), r3.upcast(), r4.upcast()], |p| p.time_shift_enabled);

    let g = ui::group(&b, "Span", Some("Only use lines in this part of each episode, e.g. to skip the opening and ending."));
    ui::row_with(&g, "Limit to a span", "", &ui::switch(st, |p| &mut p.span.enabled));
    let s1 = ui::row_with(&g, "Start", "h:mm:ss", &ui::time_entry(st, |p| &mut p.span.value.0));
    let s2 = ui::row_with(&g, "End", "h:mm:ss", &ui::time_entry(st, |p| &mut p.span.value.1));
    sensitive_when(st, vec![s1.row.upcast(), s2.row.upcast()], |p| p.span.enabled);
    pg
}

// ── Audio / snapshots / video ──────────────────────────────────────────

fn pad_rows(st: &Rc<AppState>, list: &gtk::ListBox, en: Lens<bool>, start: Lens<i64>, end: Lens<i64>) -> Vec<gtk::Widget> {
    let a = ui::row_with(list, "Pad clips", "Add a little time before and after each line.", &ui::switch(st, en));
    let b = ui::row_with(list, "Before", "Milliseconds", &ui::spin(st, start, 0.0, 10_000.0, 10.0));
    let c = ui::row_with(list, "After", "Milliseconds", &ui::spin(st, end, 0.0, 10_000.0, 10.0));
    vec![a.row.upcast(), b.row.upcast(), c.row.upcast()]
}

fn page_audio(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Audio", "One audio clip per card, cut from the episode's audio.");
    let g0 = ui::group(&b, "", None);
    ui::row_with(&g0, "Create audio clips", "", &ui::switch(st, |p| &mut p.audio.enabled));

    let g1 = ui::group(&b, "Source", None);
    ui::row_with(
        &g1,
        "Take audio from",
        "",
        &ui::dropdown(st, |p| &mut p.audio.source, vec![(AudioSource::Video, "The video files".into()), (AudioSource::Files, "Separate audio files".into())]),
    );
    let (fr, body, badge) = ui::wide_row(&g1, "Audio files", "One per episode, e.g. extracted .mka/.flac tracks.");
    body.append(&ui::path_picker(st, &c.win, |p| &mut p.audio.pattern, Pick::Files, "Choose Audio Files", Some(AUDIO_FILTER)).0);
    visible_when(st, fr.upcast(), |p| p.audio.source == AudioSource::Files);
    let s = st.clone();
    watch_first_file(st, |p| &mut p.audio.pattern, AUDIO_EXTS, move |_, n| {
        count_badge(&badge, &s.get(|p| &mut p.audio.pattern), n, "file", Some(subs1_count(&s)));
    });

    let g2 = ui::group(&b, "Encoding", None);
    ui::row_with(
        &g2,
        "Format",
        "Opus is small and sounds great; MP3 plays everywhere.",
        &ui::dropdown(st, |p| &mut p.audio.format, AudioFormat::ALL.iter().map(|f| (*f, f.label().to_string())).collect()),
    );
    let rates = [48u32, 64, 96, 128, 160, 192, 256, 320];
    ui::row_with(&g2, "Bitrate", "", &ui::dropdown(st, |p| &mut p.audio.bitrate, rates.iter().map(|r| (*r, format!("{r} kb/s"))).collect()));
    ui::row_with(&g2, "Normalize loudness", "EBU R128 via ffmpeg's loudnorm, so every clip has a similar volume.", &ui::switch(st, |p| &mut p.audio.normalize));

    let g3 = ui::group(&b, "Padding", None);
    let pads = pad_rows(st, &g3, |p| &mut p.audio.pad.enabled, |p| &mut p.audio.pad.start_ms, |p| &mut p.audio.pad.end_ms);
    sensitive_when(st, vec![g1.upcast(), g2.upcast(), g3.upcast()], |p| p.audio.enabled);
    sensitive_when(st, pads[1..].to_vec(), |p| p.audio.pad.enabled);
    pg
}

fn size_crop_rows(
    st: &Rc<AppState>,
    b: &gtk::Box,
    w: Lens<u32>,
    h: Lens<u32>,
    ct: Lens<u32>,
    cb: Lens<u32>,
    cl: Lens<u32>,
    cr: Lens<u32>,
) -> Vec<gtk::Widget> {
    let g = ui::group(b, "Size", Some("Set the height to 0 to keep the video's aspect ratio."));
    ui::row_with(&g, "Width", "Pixels", &ui::spin(st, w, 16.0, 3840.0, 2.0));
    ui::row_with(&g, "Height", "Pixels · 0 = automatic", &ui::spin(st, h, 0.0, 2160.0, 2.0));
    let g2 = ui::group(b, "Crop", Some("Cut pixels off the source video first, e.g. the bottom to hide burned-in subtitles."));
    let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for (label, lens) in [("Top", ct), ("Bottom", cb), ("Left", cl), ("Right", cr)] {
        hb.append(&gtk::Label::new(Some(label)));
        hb.append(&ui::spin(st, lens, 0.0, 2000.0, 2.0));
    }
    ui::row_with(&g2, "Crop", "Source pixels", &hb);
    vec![g.upcast(), g2.upcast()]
}

fn page_snapshots(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Snapshots", "A still image from the middle of each line.");
    let g0 = ui::group(&b, "", None);
    ui::row_with(&g0, "Create snapshots", "", &ui::switch(st, |p| &mut p.snapshots.enabled));
    let mut w = size_crop_rows(
        st,
        &b,
        |p| &mut p.snapshots.width,
        |p| &mut p.snapshots.height,
        |p| &mut p.snapshots.crop.top,
        |p| &mut p.snapshots.crop.bottom,
        |p| &mut p.snapshots.crop.left,
        |p| &mut p.snapshots.crop.right,
    );
    let g = ui::group(&b, "Quality", Some("The image format follows the snapshot file name in Preferences (.jpg by default; .webp also works)."));
    ui::row_with(&g, "JPEG quality", "2 = best, 31 = smallest", &ui::spin(st, |p| &mut p.snapshots.quality, 2.0, 31.0, 1.0));
    w.push(g.upcast());
    sensitive_when(st, w, |p| p.snapshots.enabled);
    pg
}

fn page_video(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Video Clips", "A short video per card. Off by default — they take more time and space.");
    let g0 = ui::group(&b, "", None);
    ui::row_with(&g0, "Create video clips", "", &ui::switch(st, |p| &mut p.video_clips.enabled));
    let g = ui::group(&b, "Encoding", None);
    ui::row_with(
        &g,
        "Format",
        "MP4 plays on AnkiDroid, AnkiMobile and desktop.",
        &ui::dropdown(st, |p| &mut p.video_clips.codec, VideoCodec::ALL.iter().map(|f| (*f, f.label().to_string())).collect()),
    );
    ui::row_with(&g, "Video bitrate", "kb/s", &ui::spin(st, |p| &mut p.video_clips.bitrate_video, 100.0, 20000.0, 50.0));
    let rates = [64u32, 96, 128, 160, 192, 256];
    ui::row_with(&g, "Audio bitrate", "", &ui::dropdown(st, |p| &mut p.video_clips.bitrate_audio, rates.iter().map(|r| (*r, format!("{r} kb/s"))).collect()));
    let mut w = vec![g.clone().upcast::<gtk::Widget>()];
    w.extend(size_crop_rows(
        st,
        &b,
        |p| &mut p.video_clips.width,
        |p| &mut p.video_clips.height,
        |p| &mut p.video_clips.crop.top,
        |p| &mut p.video_clips.crop.bottom,
        |p| &mut p.video_clips.crop.left,
        |p| &mut p.video_clips.crop.right,
    ));
    let g3 = ui::group(&b, "Padding", None);
    let pads = pad_rows(st, &g3, |p| &mut p.video_clips.pad.enabled, |p| &mut p.video_clips.pad.start_ms, |p| &mut p.video_clips.pad.end_ms);
    w.push(g3.upcast());
    sensitive_when(st, w, |p| p.video_clips.enabled);
    sensitive_when(st, pads[1..].to_vec(), |p| p.video_clips.pad.enabled);
    pg
}

// ── Filters ─────────────────────────────────────────────────────────────

fn toggle_spin_row(
    st: &Rc<AppState>,
    list: &gtk::ListBox,
    title: &str,
    sub: &str,
    en: Lens<bool>,
    v: Lens<i64>,
    max: f64,
    step: f64,
) {
    let r = ui::row(list, title, sub);
    let sp = ui::spin(st, v, 0.0, max, step);
    r.suffix.append(&sp);
    r.suffix.append(&ui::switch(st, en));
    let s = st.clone();
    st.on_change(move || sp.set_sensitive(s.get(en)));
}

fn filter_group(st: &Rc<AppState>, b: &gtk::Box, which: Which) -> gtk::ListBox {
    macro_rules! l {
        ($f:ident $(. $g:ident)*) => {
            match which {
                Which::Subs1 => ui::lens(|p| &mut p.subs1.$f $(.$g)*),
                Which::Subs2 => ui::lens(|p| &mut p.subs2.$f $(.$g)*),
            }
        };
    }
    let name = if which == Which::Subs1 { "Subs1" } else { "Subs2" };
    let g = ui::group(b, &format!("{name} lines"), None);
    let (_, body, _) = ui::wide_row(&g, "Only lines containing", "Words separated by semicolons. Leave empty to keep everything.");
    let e = ui::entry(st, l!(include_words));
    e.set_placeholder_text(Some("word; another word"));
    body.append(&e);
    let (_, body, _) = ui::wide_row(&g, "Skip lines containing", "");
    let e = ui::entry(st, l!(exclude_words));
    e.set_placeholder_text(Some("♪; (laughs)"));
    body.append(&e);
    ui::row_with(&g, "Remove signs and karaoke", "ASS lines that start with positioning, drawing or karaoke tags.", &ui::switch(st, l!(remove_styled)));
    ui::row_with(&g, "Remove lines without a match", "Lines that don't overlap any line of the other subtitle.", &ui::switch(st, l!(remove_no_counterpart)));
    ui::row_with(&g, "Remove duplicate lines", "Keep only the first occurrence across all episodes.", &ui::switch(st, l!(exclude_duplicates)));
    toggle_spin_row(st, &g, "Skip lines shorter than", "Characters", l!(exclude_fewer_chars.enabled), l!(exclude_fewer_chars.value), 500.0, 1.0);
    toggle_spin_row(st, &g, "Skip lines shorter than", "Milliseconds (uses the timing subtitle)", l!(exclude_shorter_ms.enabled), l!(exclude_shorter_ms.value), 60_000.0, 50.0);
    toggle_spin_row(st, &g, "Skip lines longer than", "Milliseconds (uses the timing subtitle)", l!(exclude_longer_ms.enabled), l!(exclude_longer_ms.value), 600_000.0, 100.0);
    let r = ui::row(&g, "Join lines ending with", "Merge lines that end in one of these characters with the next line.");
    let chars = ui::entry(st, l!(join_sentences.value));
    chars.set_width_chars(8);
    chars.set_hexpand(false);
    r.suffix.append(&chars);
    r.suffix.append(&ui::switch(st, l!(join_sentences.enabled)));
    g
}

fn page_filters(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Filters", "Choose which lines become cards. Use Preview to see the result.");
    filter_group(st, &b, Which::Subs1);
    let g2 = filter_group(st, &b, Which::Subs2);
    sensitive_when(st, vec![g2.upcast()], |p| p.has_subs2());

    let g = ui::group(&b, "Actors", Some("ASS and WebVTT subtitles can name the speaker of each line."));
    ui::row_with(&g, "Actor names from", "", &ui::dropdown(st, |p| &mut p.actors_from, vec![(Which::Subs1, "Subs1".into()), (Which::Subs2, "Subs2".into())]));
    let (_, body, _) = ui::wide_row(&g, "Only these actors", "Semicolon-separated. Empty = everyone.");
    let e = gtk::Entry::builder().hexpand(true).placeholder_text("Taro; Hanako").build();
    e.set_text(&st.project.borrow().actors.join("; "));
    let s = st.clone();
    e.connect_changed(move |e| {
        let v: Vec<String> = e.text().split(';').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
        if s.project.borrow().actors != v {
            s.update(|p| p.actors = v);
        }
    });
    let (s, w) = (st.clone(), e.downgrade());
    st.on_load(move || {
        if let Some(e) = w.upgrade() {
            e.set_text(&s.project.borrow().actors.join("; "));
        }
    });
    body.append(&e);
    let detect = gtk::MenuButton::builder().label("Detect").tooltip_text("List the actors in the first episode").build();
    let pop = gtk::Popover::new();
    let pop_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).margin_top(6).margin_bottom(6).margin_start(6).margin_end(6).build();
    pop.set_child(Some(&pop_box));
    detect.set_popover(Some(&pop));
    body.append(&detect);
    let (s, ew, pb, win) = (st.clone(), e.downgrade(), pop_box.clone(), c.win.clone());
    pop.connect_show(move |_| {
        while let Some(ch) = pb.first_child() {
            pb.remove(&ch);
        }
        pb.append(&gtk::Label::new(Some("Reading subtitles…")));
        let project = s.snapshot();
        let (pb2, ew2, _win2) = (pb.clone(), ew.clone(), win.clone());
        job::background(
            move || -> Result<Vec<String>, String> {
                let mut q = project.clone();
                q.audio.enabled = false;
                q.snapshots.enabled = false;
                q.video_clips.enabled = false;
                let inp = pipeline::resolve_inputs(&q).map_err(|e| e.to_string())?;
                let (file, ss) = match project.actors_from {
                    Which::Subs1 => (inp.subs1.first().cloned(), project.subs1.clone()),
                    Which::Subs2 => (inp.subs2.first().cloned(), project.subs2.clone()),
                };
                let file = file.ok_or("No subtitle file")?;
                let lines = pipeline::read_track(&file, &ss, Default::default(), None).map_err(|e| e.to_string())?;
                let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
                for l in lines {
                    if !l.actor.trim().is_empty() {
                        *counts.entry(l.actor.trim().to_string()).or_default() += 1;
                    }
                }
                let mut v: Vec<(String, usize)> = counts.into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1));
                Ok(v.into_iter().map(|(a, n)| format!("{a}\u{1f}{n}")).collect())
            },
            move |r| {
                while let Some(ch) = pb2.first_child() {
                    pb2.remove(&ch);
                }
                match r {
                    Ok(v) if v.is_empty() => pb2.append(&gtk::Label::new(Some("No actor names in this subtitle."))),
                    Ok(v) => {
                        let current: Vec<String> = ew2.upgrade().map(|e| e.text().split(';').map(|a| a.trim().to_lowercase()).collect()).unwrap_or_default();
                        let sw = gtk::ScrolledWindow::builder().max_content_height(320).propagate_natural_height(true).hscrollbar_policy(gtk::PolicyType::Never).build();
                        let inner = gtk::Box::new(gtk::Orientation::Vertical, 2);
                        sw.set_child(Some(&inner));
                        for item in v {
                            let (name, n) = item.split_once('\u{1f}').unwrap_or((&item, ""));
                            let cb = gtk::CheckButton::with_label(&format!("{name}  ({n} lines)"));
                            cb.set_active(current.contains(&name.to_lowercase()));
                            let (ew3, name) = (ew2.clone(), name.to_string());
                            cb.connect_toggled(move |cb| {
                                let Some(e) = ew3.upgrade() else { return };
                                let mut list: Vec<String> = e.text().split(';').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
                                list.retain(|a| !a.eq_ignore_ascii_case(&name));
                                if cb.is_active() {
                                    list.push(name.clone());
                                }
                                e.set_text(&list.join("; "));
                            });
                            inner.append(&cb);
                        }
                        pb2.append(&sw);
                    }
                    Err(e) => pb2.append(&gtk::Label::builder().label(e).wrap(true).max_width_chars(40).build()),
                }
            },
        );
    });

    let g = ui::group(&b, "Language", None);
    ui::row_with(&g, "Japanese: only lines with kanji", "Skip lines written only in kana or Latin letters.", &ui::switch(st, |p| &mut p.kanji_only));
    pg
}

// ── Context ─────────────────────────────────────────────────────────────

fn page_context(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let st = &c.st;
    let (pg, b) = ui::page("Context", "Add the lines before and after each card as extra fields, so you can see what led up to a sentence.");
    for leading in [true, false] {
        macro_rules! l {
            ($f:ident) => {
                if leading {
                    ui::lens(|p| &mut p.context_leading.$f)
                } else {
                    ui::lens(|p| &mut p.context_trailing.$f)
                }
            };
        }
        let g = ui::group(&b, if leading { "Lines before" } else { "Lines after" }, None);
        ui::row_with(&g, "Number of lines", "0 = none", &ui::spin(st, l!(count), 0.0, 20.0, 1.0));
        ui::row_with(&g, "Maximum gap", "Seconds · lines further away are left empty · 0 = no limit", &ui::spin(st, l!(range_secs), 0.0, 3600.0, 1.0));
        ui::row_with(&g, "Include audio clips", "", &ui::switch(st, l!(include_audio)));
        ui::row_with(&g, "Include snapshots", "", &ui::switch(st, l!(include_snapshots)));
        ui::row_with(&g, "Include video clips", "", &ui::switch(st, l!(include_video)));
    }
    pg
}

// ── Tools ───────────────────────────────────────────────────────────────

fn tool_card(icon: &str, title: &str, desc: &str, button: &str, on_click: impl Fn() + 'static) -> gtk::Box {
    let card = gtk::Box::builder().spacing(16).css_classes(["s2s-tool-card"]).margin_top(12).build();
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(36);
    img.set_valign(gtk::Align::Start);
    card.append(&img);
    let vb = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).hexpand(true).build();
    vb.append(&gtk::Label::builder().label(title).xalign(0.0).css_classes(["heading"]).build());
    vb.append(&gtk::Label::builder().label(desc).xalign(0.0).wrap(true).max_width_chars(60).css_classes(["s2s-row-subtitle"]).build());
    card.append(&vb);
    let btn = gtk::Button::builder().label(button).valign(gtk::Align::Center).build();
    btn.connect_clicked(move |_| on_click());
    card.append(&btn);
    card
}

fn page_tools(c: &Rc<Ctx>) -> gtk::ScrolledWindow {
    let (pg, b) = ui::page("Tools", "Extras that reuse the subtitles and videos from the Source page.");
    let c2 = c.clone();
    b.append(&tool_card(
        "s2s-subtitles-symbolic",
        "Dueling subtitles",
        "Create .ass files that show both languages at once, for watching with mpv or any player. Optionally writes a plain-text transcript.",
        "Open…",
        move || {
            let _ = WidgetExt::activate_action(&c2.win, "win.dueling", None);
        },
    ));
    let c2 = c.clone();
    b.append(&tool_card(
        "s2s-audio-symbolic",
        "Extract audio",
        "Turn episodes into MP3/Opus files for listening practice, whole or in chunks, with the subtitles as lyrics.",
        "Open…",
        move || {
            let _ = WidgetExt::activate_action(&c2.win, "win.extract-audio", None);
        },
    ));
    let c2 = c.clone();
    b.append(&tool_card(
        "s2s-preview-symbolic",
        "Preview lines",
        "See which lines will become cards, toggle them, fix typos and listen to the audio before generating.",
        "Preview…",
        move || {
            let _ = WidgetExt::activate_action(&c2.win, "win.preview", None);
        },
    ));
    pg
}
