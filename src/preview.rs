//! Preview window: inspect, toggle and edit matched lines before generating.

use crate::job;
use crate::ui::{self, AppState};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use subs2srs_core::ffmpeg;
use subs2srs_core::model::Episode;
use subs2srs_core::pipeline;
use subs2srs_core::progress::Progress;
use subs2srs_core::settings::{AudioSource, Project};
use subs2srs_core::time;

type OnGenerate = Rc<dyn Fn(Vec<Episode>)>;

thread_local! {
    static CURRENT: RefCell<Option<Rc<Preview>>> = const { RefCell::new(None) };
}

struct Preview {
    st: Rc<AppState>,
    win: gtk::Window,
    data: RefCell<Vec<Episode>>,
    project: RefCell<Project>,
    store: gio::ListStore,
    sel: gtk::SingleSelection,
    episode_dd: gtk::DropDown,
    episode_model: gtk::StringList,
    search: gtk::SearchEntry,
    stats: gtk::Label,
    stack: gtk::Stack,
    loading_label: gtk::Label,
    picture: gtk::Picture,
    snap_info: gtk::Label,
    time_label: gtk::Label,
    s1: gtk::Entry,
    s2: gtk::Entry,
    active: gtk::Switch,
    side: gtk::Box,
    snap_gen: Cell<u64>,
    syncing: Cell<bool>,
    player: RefCell<Option<Child>>,
    tmp: PathBuf,
}

fn key(o: &glib::Object) -> Option<(usize, usize)> {
    o.downcast_ref::<glib::BoxedAnyObject>().map(|b| *b.borrow::<(usize, usize)>())
}

pub fn open(parent: &gtk::ApplicationWindow, st: Rc<AppState>, on_generate: OnGenerate) {
    let win = gtk::Window::builder().title("Preview").transient_for(parent).default_width(1100).default_height(700).build();
    let header = gtk::HeaderBar::new();
    let reload = gtk::Button::builder().icon_name("view-refresh-symbolic").tooltip_text("Reload with the current settings").build();
    header.pack_start(&reload);
    let gen = gtk::Button::builder().label("Generate").css_classes(["suggested-action"]).tooltip_text("Create the deck from these lines").build();
    header.pack_end(&gen);
    win.set_titlebar(Some(&header));

    // Toolbar
    let episode_model = gtk::StringList::new(&[]);
    let episode_dd = gtk::DropDown::new(Some(episode_model.clone()), None::<gtk::Expression>);
    let search = gtk::SearchEntry::builder().placeholder_text("Search lines").hexpand(true).build();
    let stats = gtk::Label::builder().css_classes(["s2s-info"]).build();
    let bulk = gtk::MenuButton::builder().icon_name("view-more-symbolic").tooltip_text("Change all shown lines").build();
    let bulk_menu = gio::Menu::new();
    bulk_menu.append(Some("Activate shown lines"), Some("preview.all-on"));
    bulk_menu.append(Some("Deactivate shown lines"), Some("preview.all-off"));
    bulk_menu.append(Some("Invert shown lines"), Some("preview.invert"));
    bulk.set_menu_model(Some(&bulk_menu));
    let toolbar = gtk::Box::builder().spacing(8).margin_top(8).margin_bottom(8).margin_start(12).margin_end(12).build();
    toolbar.append(&episode_dd);
    toolbar.append(&search);
    toolbar.append(&stats);
    toolbar.append(&bulk);

    // Line list
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let sel = gtk::SingleSelection::new(Some(store.clone()));
    let cv = gtk::ColumnView::builder().model(&sel).show_row_separators(true).css_classes(["s2s-lines"]).reorderable(false).build();
    let list_scroll = gtk::ScrolledWindow::builder().child(&cv).hexpand(true).vexpand(true).build();

    let loading_label = gtk::Label::builder().label("Reading subtitles…").css_classes(["s2s-empty"]).build();
    let spinner = gtk::Spinner::builder().spinning(true).width_request(32).height_request(32).build();
    let loading = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).valign(gtk::Align::Center).halign(gtk::Align::Center).build();
    loading.append(&spinner);
    loading.append(&loading_label);
    let stack = gtk::Stack::new();
    stack.add_named(&loading, Some("loading"));
    stack.add_named(&list_scroll, Some("list"));

    // Side panel
    let picture = gtk::Picture::builder().content_fit(gtk::ContentFit::Contain).height_request(200).css_classes(["s2s-preview-snap"]).build();
    let snap_info = gtk::Label::builder().css_classes(["s2s-info"]).wrap(true).build();
    let time_label = gtk::Label::builder().xalign(0.0).css_classes(["heading"]).build();
    let s1 = gtk::Entry::builder().placeholder_text("Subs1").build();
    let s2 = gtk::Entry::builder().placeholder_text("Subs2").build();
    let active = gtk::Switch::builder().valign(gtk::Align::Center).build();
    let play = gtk::Button::builder().child(&ui::icon_label("media-playback-start-symbolic", "Play audio")).build();
    let side = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .width_request(340)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .sensitive(false)
        .build();
    side.append(&picture);
    side.append(&snap_info);
    side.append(&time_label);
    side.append(&gtk::Label::builder().label("Subs1").xalign(0.0).css_classes(["s2s-row-subtitle"]).build());
    side.append(&s1);
    side.append(&gtk::Label::builder().label("Subs2").xalign(0.0).css_classes(["s2s-row-subtitle"]).build());
    side.append(&s2);
    let act_row = gtk::Box::builder().spacing(8).margin_top(6).build();
    act_row.append(&gtk::Label::builder().label("Make a card from this line").hexpand(true).xalign(0.0).build());
    act_row.append(&active);
    side.append(&act_row);
    side.append(&play);
    side.append(
        &gtk::Label::builder()
            .label("Double-click or press Enter on a line to toggle it. Edits are used when you press Generate here.")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["s2s-row-subtitle"])
            .margin_top(12)
            .build(),
    );

    let side_scroll = gtk::ScrolledWindow::builder().child(&side).hscrollbar_policy(gtk::PolicyType::Never).propagate_natural_width(true).build();
    let paned = gtk::Paned::builder().orientation(gtk::Orientation::Horizontal).start_child(&stack).end_child(&side_scroll).resize_end_child(false).shrink_end_child(false).build();
    paned.set_position(720);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&toolbar);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&paned);
    paned.set_vexpand(true);
    win.set_child(Some(&root));

    let tmp = std::env::temp_dir().join(format!("subs2srs-preview-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);

    let pv = Rc::new(Preview {
        st: st.clone(),
        win: win.clone(),
        data: RefCell::new(Vec::new()),
        project: RefCell::new(st.snapshot()),
        store,
        sel,
        episode_dd,
        episode_model,
        search,
        stats,
        stack,
        loading_label,
        picture,
        snap_info,
        time_label,
        s1,
        s2,
        active,
        side,
        snap_gen: Cell::new(0),
        syncing: Cell::new(false),
        player: RefCell::new(None),
        tmp,
    });

    build_columns(&pv, &cv);

    // Signals
    let w = Rc::downgrade(&pv);
    pv.episode_dd.connect_selected_notify(move |_| {
        if let Some(pv) = w.upgrade() {
            if !pv.syncing.get() {
                refill(&pv)
            }
        }
    });
    let w = Rc::downgrade(&pv);
    pv.search.connect_search_changed(move |_| {
        if let Some(pv) = w.upgrade() {
            refill(&pv)
        }
    });
    let w = Rc::downgrade(&pv);
    pv.sel.connect_selected_notify(move |_| {
        if let Some(pv) = w.upgrade() {
            show_selected(&pv)
        }
    });
    let w = Rc::downgrade(&pv);
    cv.connect_activate(move |_, pos| {
        if let Some(pv) = w.upgrade() {
            if let Some(k) = pv.store.item(pos).and_then(|o| key(&o)) {
                let v = !pv.data.borrow()[k.0].lines[k.1].active;
                set_active(&pv, k, v, Some(pos));
            }
        }
    });
    let w = Rc::downgrade(&pv);
    pv.active.connect_active_notify(move |sw| {
        if let Some(pv) = w.upgrade() {
            if pv.syncing.get() {
                return;
            }
            if let Some((k, pos)) = selected(&pv) {
                set_active(&pv, k, sw.is_active(), Some(pos));
            }
        }
    });
    for (entry, is1) in [(pv.s1.clone(), true), (pv.s2.clone(), false)] {
        let w = Rc::downgrade(&pv);
        entry.connect_changed(move |e| {
            let Some(pv) = w.upgrade() else { return };
            if pv.syncing.get() {
                return;
            }
            if let Some(((ep, i), pos)) = selected(&pv) {
                {
                    let mut d = pv.data.borrow_mut();
                    let l = &mut d[ep].lines[i];
                    if is1 {
                        l.subs1.text = e.text().to_string()
                    } else {
                        l.subs2.text = e.text().to_string()
                    }
                }
                pv.store.items_changed(pos, 1, 1);
            }
        });
    }
    let w = Rc::downgrade(&pv);
    play.connect_clicked(move |_| {
        if let Some(pv) = w.upgrade() {
            play_audio(&pv)
        }
    });
    let w = Rc::downgrade(&pv);
    reload.connect_clicked(move |_| {
        if let Some(pv) = w.upgrade() {
            *pv.project.borrow_mut() = pv.st.snapshot();
            load(&pv);
        }
    });
    let w = Rc::downgrade(&pv);
    gen.connect_clicked(move |_| {
        if let Some(pv) = w.upgrade() {
            let eps = pv.data.borrow().clone();
            if eps.is_empty() {
                return;
            }
            pv.win.close();
            on_generate(eps);
        }
    });

    let actions = gio::SimpleActionGroup::new();
    for (name, mode) in [("all-on", 0), ("all-off", 1), ("invert", 2)] {
        let a = gio::SimpleAction::new(name, None);
        let w = Rc::downgrade(&pv);
        a.connect_activate(move |_, _| {
            let Some(pv) = w.upgrade() else { return };
            {
                let mut d = pv.data.borrow_mut();
                for i in 0..pv.store.n_items() {
                    if let Some((e, l)) = pv.store.item(i).and_then(|o| key(&o)) {
                        let line = &mut d[e].lines[l];
                        line.active = match mode {
                            0 => true,
                            1 => false,
                            _ => !line.active,
                        };
                    }
                }
            }
            let n = pv.store.n_items();
            pv.store.items_changed(0, n, n);
            update_stats(&pv);
            show_selected(&pv);
        });
        actions.add_action(&a);
    }
    win.insert_action_group("preview", Some(&actions));

    let w = Rc::downgrade(&pv);
    win.connect_close_request(move |_| {
        if let Some(pv) = w.upgrade() {
            if let Some(mut c) = pv.player.borrow_mut().take() {
                let _ = c.kill();
            }
            let _ = std::fs::remove_dir_all(&pv.tmp);
        }
        CURRENT.with(|c| {
            let mine = c.borrow().as_ref().zip(w.upgrade()).map(|(a, b)| Rc::ptr_eq(a, &b)).unwrap_or(false);
            if mine {
                c.borrow_mut().take();
            }
        });
        glib::Propagation::Proceed
    });
    // Keep the Preview alive while its window is open.
    CURRENT.with(|c| *c.borrow_mut() = Some(pv.clone()));

    win.present();
    load(&pv);
}

fn label_factory(pv: &Rc<Preview>, text: fn(&Episode, usize) -> String, xalign: f32, wrap: bool) -> gtk::SignalListItemFactory {
    let f = gtk::SignalListItemFactory::new();
    f.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let l = gtk::Label::builder().xalign(xalign).ellipsize(if wrap { gtk::pango::EllipsizeMode::End } else { gtk::pango::EllipsizeMode::None }).build();
        item.set_child(Some(&l));
    });
    let w = Rc::downgrade(pv);
    f.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let (Some(pv), Some(k)) = (w.upgrade(), item.item().and_then(|o| key(&o))) else { return };
        let Some(l) = item.child().and_downcast::<gtk::Label>() else { return };
        let d = pv.data.borrow();
        let ep = &d[k.0];
        let c = &ep.lines[k.1];
        let t = text(ep, k.1);
        l.set_text(&t);
        l.set_tooltip_text(if wrap { Some(t.as_str()) } else { None });
        if c.active {
            l.remove_css_class("s2s-dim");
        } else {
            l.add_css_class("s2s-dim");
        }
    });
    f
}

fn build_columns(pv: &Rc<Preview>, cv: &gtk::ColumnView) {
    // Active toggle
    let f = gtk::SignalListItemFactory::new();
    let w = Rc::downgrade(pv);
    f.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let cb = gtk::CheckButton::new();
        let (w2, li) = (w.clone(), item.downgrade());
        cb.connect_toggled(move |cb| {
            let (Some(pv), Some(li)) = (w2.upgrade(), li.upgrade()) else { return };
            let Some(k) = li.item().and_then(|o| key(&o)) else { return };
            let cur = pv.data.borrow()[k.0].lines[k.1].active;
            if cur != cb.is_active() {
                let pos = li.position();
                // Defer the rebind so we don't rebuild this row inside its own handler.
                let pv2 = pv.clone();
                let v = cb.is_active();
                glib::idle_add_local_once(move || set_active(&pv2, k, v, Some(pos)));
            }
        });
        item.set_child(Some(&cb));
    });
    let w = Rc::downgrade(pv);
    f.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let (Some(pv), Some(k)) = (w.upgrade(), item.item().and_then(|o| key(&o))) else { return };
        if let Some(cb) = item.child().and_downcast::<gtk::CheckButton>() {
            cb.set_active(pv.data.borrow()[k.0].lines[k.1].active);
        }
    });
    let col = gtk::ColumnViewColumn::new(None, Some(f));
    cv.append_column(&col);

    let cols: Vec<(&str, fn(&Episode, usize) -> String, f32, bool, i32)> = vec![
        ("Ep", |e, _| e.number.to_string(), 1.0, false, 40),
        ("Start", |e, i| time::to_hms_ms(e.lines[i].start()), 0.0, false, 100),
        ("Length", |e, i| format!("{:.1}s", (e.lines[i].end() - e.lines[i].start()) as f64 / 1000.0), 1.0, false, 60),
        ("Subs1", |e, i| e.lines[i].subs1.text.clone(), 0.0, true, -1),
        ("Subs2", |e, i| e.lines[i].subs2.text.clone(), 0.0, true, -1),
    ];
    for (title, text, xalign, wrap, width) in cols {
        let col = gtk::ColumnViewColumn::new(Some(title), Some(label_factory(pv, text, xalign, wrap)));
        col.set_resizable(true);
        if width > 0 {
            col.set_fixed_width(width);
        } else {
            col.set_expand(true);
        }
        cv.append_column(&col);
    }
}

fn selected(pv: &Preview) -> Option<((usize, usize), u32)> {
    let pos = pv.sel.selected();
    if pos == gtk::INVALID_LIST_POSITION {
        return None;
    }
    pv.store.item(pos).and_then(|o| key(&o)).map(|k| (k, pos))
}

fn set_active(pv: &Rc<Preview>, k: (usize, usize), v: bool, pos: Option<u32>) {
    pv.data.borrow_mut()[k.0].lines[k.1].active = v;
    if let Some(p) = pos {
        pv.store.items_changed(p, 1, 1);
    }
    update_stats(pv);
    if selected(pv).map(|(sk, _)| sk == k).unwrap_or(false) {
        pv.syncing.set(true);
        pv.active.set_active(v);
        pv.syncing.set(false);
    }
}

fn load(pv: &Rc<Preview>) {
    pv.stack.set_visible_child_name("loading");
    pv.loading_label.set_text("Reading subtitles…");
    pv.side.set_sensitive(false);
    let project = pv.project.borrow().clone();
    let progress = Arc::new(Progress::new());
    let (w1, w2) = (Rc::downgrade(pv), Rc::downgrade(pv));
    job::run(
        progress,
        move |p| pipeline::prepare(&project, p).map(|(_, e)| e),
        move |s, _| {
            if let Some(pv) = w1.upgrade() {
                if !s.detail.is_empty() {
                    pv.loading_label.set_text(&s.detail);
                }
            }
        },
        move |r| {
            let Some(pv) = w2.upgrade() else { return };
            match r {
                Ok(eps) => {
                    pv.syncing.set(true);
                    let mut names = vec![format!("All episodes ({})", eps.len())];
                    names.extend(eps.iter().map(|e| format!("Episode {}", e.number)));
                    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                    pv.episode_model.splice(0, pv.episode_model.n_items(), &refs);
                    pv.episode_dd.set_selected(0);
                    pv.syncing.set(false);
                    *pv.data.borrow_mut() = eps;
                    refill(&pv);
                    pv.stack.set_visible_child_name("list");
                }
                Err(e) => {
                    pv.loading_label.set_text(&e.to_string());
                    pv.loading_label.set_wrap(true);
                    pv.loading_label.set_max_width_chars(70);
                }
            }
        },
    );
}

fn refill(pv: &Rc<Preview>) {
    let ep_sel = pv.episode_dd.selected();
    let q = pv.search.text().to_lowercase();
    let d = pv.data.borrow();
    let items: Vec<glib::BoxedAnyObject> = d
        .iter()
        .enumerate()
        .filter(|(i, _)| ep_sel == 0 || ep_sel == gtk::INVALID_LIST_POSITION || *i as u32 == ep_sel - 1)
        .flat_map(|(ei, ep)| {
            let q = q.clone();
            ep.lines.iter().enumerate().filter(move |(_, c)| q.is_empty() || c.subs1.text.to_lowercase().contains(&q) || c.subs2.text.to_lowercase().contains(&q)).map(move |(li, _)| glib::BoxedAnyObject::new((ei, li)))
        })
        .collect();
    drop(d);
    pv.store.remove_all();
    pv.store.extend_from_slice(&items);
    update_stats(pv);
    show_selected(pv);
}

fn update_stats(pv: &Preview) {
    let d = pv.data.borrow();
    let total: usize = d.iter().map(|e| e.lines.len()).sum();
    let active: usize = d.iter().map(|e| e.lines.iter().filter(|c| c.active).count()).sum();
    pv.stats.set_text(&format!("{active} of {total} lines will become cards · {} shown", pv.store.n_items()));
}

fn show_selected(pv: &Rc<Preview>) {
    let Some(((ep, i), _)) = selected(pv) else {
        pv.side.set_sensitive(false);
        return;
    };
    pv.side.set_sensitive(true);
    let (c, video) = {
        let d = pv.data.borrow();
        (d[ep].lines[i].clone(), d[ep].video_file.clone())
    };
    pv.syncing.set(true);
    pv.s1.set_text(&c.subs1.text);
    pv.s2.set_text(&c.subs2.text);
    pv.s2.set_sensitive(pv.project.borrow().has_subs2());
    pv.active.set_active(c.active);
    pv.syncing.set(false);
    pv.time_label.set_text(&format!("{} → {}", time::to_hms_ms(c.start()), time::to_hms_ms(c.end())));

    // Snapshot of the middle of the line.
    let gen = pv.snap_gen.get() + 1;
    pv.snap_gen.set(gen);
    let Some(video) = video else {
        pv.picture.set_paintable(None::<&gtk::gdk::Paintable>);
        pv.snap_info.set_text("No video selected");
        return;
    };
    pv.snap_info.set_text("");
    let snaps = pv.project.borrow().snapshots.clone();
    let out = pv.tmp.join(format!("snap-{gen}.jpg"));
    let mid = time::midpoint(c.start(), c.end());
    let w = Rc::downgrade(pv);
    job::background(
        move || {
            let args: Vec<String> = vec![
                "-y".into(),
                "-ss".into(),
                ffmpeg::ss(mid),
                "-i".into(),
                video,
                "-map".into(),
                "0:v:0".into(),
                "-frames:v".into(),
                "1".into(),
                "-vf".into(),
                ffmpeg::video_filter(snaps.width.max(320), snaps.height, &snaps.crop, false),
                "-q:v".into(),
                "3".into(),
                out.display().to_string(),
            ];
            ffmpeg::run(&args, None, None).map(|_| out)
        },
        move |r| {
            let Some(pv) = w.upgrade() else { return };
            if pv.snap_gen.get() != gen {
                return;
            }
            match r {
                Ok(path) => {
                    pv.picture.set_filename(Some(&path));
                }
                Err(e) => pv.snap_info.set_text(e.to_string().lines().next().unwrap_or("")),
            }
        },
    );
}

fn player_command(custom: &str) -> Option<Vec<String>> {
    if !custom.trim().is_empty() {
        return Some(custom.split_whitespace().map(String::from).collect());
    }
    let candidates: [&[&str]; 5] = [
        &["mpv", "--no-video", "--really-quiet", "--no-terminal"],
        &["ffplay", "-nodisp", "-autoexit", "-loglevel", "quiet"],
        &["pw-play"],
        &["paplay"],
        &["aplay", "-q"],
    ];
    candidates.iter().find(|c| ffmpeg::which(c[0]).is_some()).map(|c| c.iter().map(|s| s.to_string()).collect())
}

fn play_audio(pv: &Rc<Preview>) {
    let Some(((ep, i), _)) = selected(pv) else { return };
    let p = pv.project.borrow().clone();
    let (c, file) = {
        let d = pv.data.borrow();
        let e = &d[ep];
        let f = match p.audio.source {
            AudioSource::Video => e.video_file.clone(),
            AudioSource::Files => e.audio_file.clone(),
        };
        (e.lines[i].clone(), f)
    };
    let Some(file) = file else {
        ui::alert(&pv.win, "No audio source", "Choose the video (or audio files) on the Source page first.");
        return;
    };
    let Some(cmd) = player_command(&pv.st.prefs.borrow().audio_player) else {
        ui::alert(&pv.win, "No audio player found", "Install mpv or ffplay, or set an audio player command in Preferences.");
        return;
    };
    if let Some(mut old) = pv.player.borrow_mut().take() {
        let _ = old.kill();
    }
    let (s, e) = p.audio.pad.apply(c.start(), c.end());
    let map = match p.audio.source {
        AudioSource::Video => format!("0:a:{}", p.audio_stream),
        AudioSource::Files => "0:a:0".into(),
    };
    let out = pv.tmp.join("clip.wav");
    let w = Rc::downgrade(pv);
    job::background(
        move || {
            let args: Vec<String> = vec![
                "-y".into(),
                "-ss".into(),
                ffmpeg::ss(s),
                "-i".into(),
                file,
                "-t".into(),
                ffmpeg::ss(e - s),
                "-map".into(),
                map,
                "-vn".into(),
                "-ac".into(),
                "2".into(),
                out.display().to_string(),
            ];
            ffmpeg::run(&args, None, None).map(|_| out)
        },
        move |r| {
            let Some(pv) = w.upgrade() else { return };
            match r {
                Ok(path) => {
                    let child = Command::new(&cmd[0]).args(&cmd[1..]).arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
                    match child {
                        Ok(ch) => *pv.player.borrow_mut() = Some(ch),
                        Err(e) => ui::alert(&pv.win, "Could not start the audio player", &e.to_string()),
                    }
                }
                Err(e) => ui::alert(&pv.win, "Could not cut the audio", &e.to_string()),
            }
        },
    );
}
