//! Shared state, two-way bindings between widgets and the `Project`, and
//! builders for the "card list" rows used on every page.

use gtk::prelude::*;
use gtk::{gdk, gio};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use subs2srs_core::settings::{Prefs, Project};

/// A field of the project: `|p| &mut p.deck_name`.
pub type Lens<T> = for<'a> fn(&'a mut Project) -> &'a mut T;

/// Helps type inference for closures used as lenses.
pub fn lens<T>(f: Lens<T>) -> Lens<T> {
    f
}

pub struct AppState {
    pub project: RefCell<Project>,
    pub prefs: RefCell<Prefs>,
    pub path: RefCell<Option<PathBuf>>,
    pub dirty: Cell<bool>,
    loaders: RefCell<Vec<Rc<dyn Fn()>>>,
    reactors: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl AppState {
    pub fn new(project: Project, prefs: Prefs) -> Rc<Self> {
        Rc::new(AppState {
            project: RefCell::new(project),
            prefs: RefCell::new(prefs),
            path: RefCell::new(None),
            dirty: Cell::new(false),
            loaders: RefCell::new(Vec::new()),
            reactors: RefCell::new(Vec::new()),
        })
    }

    pub fn get<T: Clone>(&self, lens: Lens<T>) -> T {
        lens(&mut self.project.borrow_mut()).clone()
    }

    pub fn set<T: PartialEq>(&self, lens: Lens<T>, v: T) {
        {
            let mut p = self.project.borrow_mut();
            let slot = lens(&mut p);
            if *slot == v {
                return;
            }
            *slot = v;
        }
        self.dirty.set(true);
        self.react();
    }

    /// Mutate the project with arbitrary code.
    pub fn update(&self, f: impl FnOnce(&mut Project)) {
        f(&mut self.project.borrow_mut());
        self.dirty.set(true);
        self.react();
    }

    pub fn snapshot(&self) -> Project {
        self.project.borrow().clone()
    }

    /// Called to push project values into widgets (after loading a project).
    pub fn on_load(&self, f: impl Fn() + 'static) {
        self.loaders.borrow_mut().push(Rc::new(f));
    }

    /// Called after every change (sensitivity, info labels…). Also run once now.
    pub fn on_change(&self, f: impl Fn() + 'static) {
        f();
        self.reactors.borrow_mut().push(Rc::new(f));
    }

    pub fn react(&self) {
        let list: Vec<Rc<dyn Fn()>> = self.reactors.borrow().clone();
        for f in list {
            f();
        }
    }

    pub fn load_project(&self, p: Project, path: Option<PathBuf>) {
        *self.project.borrow_mut() = p;
        *self.path.borrow_mut() = path;
        let list: Vec<Rc<dyn Fn()>> = self.loaders.borrow().clone();
        for f in list {
            f();
        }
        self.dirty.set(false);
        self.react();
    }
}

pub trait Num: Copy + PartialEq + 'static {
    fn to_f64(self) -> f64;
    fn from_f64(v: f64) -> Self;
}
impl Num for i64 {
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn from_f64(v: f64) -> Self {
        v.round() as i64
    }
}
impl Num for u32 {
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn from_f64(v: f64) -> Self {
        v.round().max(0.0) as u32
    }
}
impl Num for usize {
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn from_f64(v: f64) -> Self {
        v.round().max(0.0) as usize
    }
}

// ── Bound widgets ──────────────────────────────────────────────────────

pub fn entry(st: &Rc<AppState>, lens: Lens<String>) -> gtk::Entry {
    let e = gtk::Entry::builder().hexpand(true).valign(gtk::Align::Center).build();
    e.set_text(&st.get(lens));
    let s = st.clone();
    e.connect_changed(move |e| s.set(lens, e.text().to_string()));
    let (s, w) = (st.clone(), e.downgrade());
    st.on_load(move || {
        if let Some(e) = w.upgrade() {
            let v = s.get(lens);
            if e.text() != v {
                e.set_text(&v);
            }
        }
    });
    e
}

pub fn spin<T: Num>(st: &Rc<AppState>, lens: Lens<T>, min: f64, max: f64, step: f64) -> gtk::SpinButton {
    let sb = gtk::SpinButton::with_range(min, max, step);
    sb.set_valign(gtk::Align::Center);
    sb.set_numeric(true);
    sb.set_value(st.get(lens).to_f64());
    let s = st.clone();
    sb.connect_value_changed(move |sb| s.set(lens, T::from_f64(sb.value())));
    let (s, w) = (st.clone(), sb.downgrade());
    st.on_load(move || {
        if let Some(sb) = w.upgrade() {
            sb.set_value(s.get(lens).to_f64());
        }
    });
    sb
}

pub fn switch(st: &Rc<AppState>, lens: Lens<bool>) -> gtk::Switch {
    let sw = gtk::Switch::builder().valign(gtk::Align::Center).active(st.get(lens)).build();
    let s = st.clone();
    sw.connect_active_notify(move |sw| s.set(lens, sw.is_active()));
    let (s, w) = (st.clone(), sw.downgrade());
    st.on_load(move || {
        if let Some(sw) = w.upgrade() {
            sw.set_active(s.get(lens));
        }
    });
    sw
}

pub fn dropdown<T: PartialEq + Copy + 'static>(st: &Rc<AppState>, lens: Lens<T>, options: Vec<(T, String)>) -> gtk::DropDown {
    let labels: Vec<&str> = options.iter().map(|(_, l)| l.as_str()).collect();
    let dd = gtk::DropDown::from_strings(&labels);
    dd.set_valign(gtk::Align::Center);
    let options = Rc::new(options);
    let pos = |opts: &[(T, String)], v: T| opts.iter().position(|(o, _)| *o == v).unwrap_or(0) as u32;
    dd.set_selected(pos(&options, st.get(lens)));
    let (s, o) = (st.clone(), options.clone());
    dd.connect_selected_notify(move |dd| {
        if let Some((v, _)) = o.get(dd.selected() as usize) {
            s.set(lens, *v);
        }
    });
    let (s, w) = (st.clone(), dd.downgrade());
    st.on_load(move || {
        if let Some(dd) = w.upgrade() {
            dd.set_selected(pos(&options, s.get(lens)));
        }
    });
    dd
}

/// `h:mm:ss` entry bound to a millisecond value.
pub fn time_entry(st: &Rc<AppState>, lens: Lens<i64>) -> gtk::Entry {
    use subs2srs_core::time;
    let e = gtk::Entry::builder().width_chars(9).max_width_chars(10).valign(gtk::Align::Center).build();
    e.set_text(&time::to_hms(st.get(lens)));
    e.set_placeholder_text(Some("h:mm:ss"));
    let s = st.clone();
    e.connect_changed(move |e| match time::parse_hms(&e.text()) {
        Some(v) => {
            e.remove_css_class("s2s-invalid");
            s.set(lens, v);
        }
        None => e.add_css_class("s2s-invalid"),
    });
    let (s, w) = (st.clone(), e.downgrade());
    st.on_load(move || {
        if let Some(e) = w.upgrade() {
            e.set_text(&time::to_hms(s.get(lens)));
        }
    });
    e
}

// ── Layout ─────────────────────────────────────────────────────────────

/// A scrollable page with a title and description. Returns (page, content box).
pub fn page(title: &str, desc: &str) -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(24)
        .margin_bottom(32)
        .margin_start(28)
        .margin_end(28)
        .halign(gtk::Align::Center)
        .width_request(640)
        .build();
    let t = gtk::Label::builder().label(title).xalign(0.0).css_classes(["s2s-page-title"]).build();
    content.append(&t);
    if !desc.is_empty() {
        let d = gtk::Label::builder().label(desc).xalign(0.0).wrap(true).max_width_chars(80).css_classes(["s2s-page-desc"]).build();
        content.append(&d);
    }
    let sw = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&content).build();
    (sw, content)
}

/// A titled card. Append rows with [`row`] & friends.
pub fn group(parent: &gtk::Box, title: &str, desc: Option<&str>) -> gtk::ListBox {
    if !title.is_empty() {
        parent.append(&gtk::Label::builder().label(title).xalign(0.0).css_classes(["s2s-group-title"]).build());
    }
    if let Some(d) = desc {
        parent.append(
            &gtk::Label::builder().label(d).xalign(0.0).wrap(true).max_width_chars(90).css_classes(["s2s-group-desc"]).build(),
        );
    } else if !title.is_empty() {
        parent.append(&gtk::Box::builder().height_request(6).build());
    }
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["s2s-card"]).build();
    list.set_show_separators(true);
    parent.append(&list);
    list
}

pub struct Row {
    pub row: gtk::ListBoxRow,
    pub subtitle: gtk::Label,
    pub suffix: gtk::Box,
}

/// Title (+ subtitle) on the left, widgets on the right.
pub fn row(list: &gtk::ListBox, title: &str, subtitle: &str) -> Row {
    let hb = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let vb = gtk::Box::builder().orientation(gtk::Orientation::Vertical).valign(gtk::Align::Center).hexpand(true).spacing(2).build();
    let t = gtk::Label::builder().label(title).xalign(0.0).wrap(true).css_classes(["s2s-row-title"]).build();
    let s = gtk::Label::builder()
        .label(subtitle)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(60)
        .css_classes(["s2s-row-subtitle"])
        .visible(!subtitle.is_empty())
        .build();
    vb.append(&t);
    vb.append(&s);
    hb.append(&vb);
    let suffix = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
    hb.append(&suffix);
    let r = gtk::ListBoxRow::builder().child(&hb).activatable(false).build();
    list.append(&r);
    Row { row: r, subtitle: s, suffix }
}

pub fn set_subtitle(l: &gtk::Label, text: &str) {
    l.set_text(text);
    l.set_visible(!text.is_empty());
}

pub fn row_with(list: &gtk::ListBox, title: &str, subtitle: &str, w: &impl IsA<gtk::Widget>) -> Row {
    let r = row(list, title, subtitle);
    r.suffix.append(w);
    r
}

/// A row whose content (entry etc.) spans the full width below the title.
pub fn wide_row(list: &gtk::ListBox, title: &str, subtitle: &str) -> (gtk::ListBoxRow, gtk::Box, gtk::Label) {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.append(&gtk::Label::builder().label(title).xalign(0.0).css_classes(["s2s-row-title"]).build());
    let info = gtk::Label::builder().xalign(1.0).hexpand(true).halign(gtk::Align::End).visible(false).build();
    head.append(&info);
    vb.append(&head);
    if !subtitle.is_empty() {
        vb.append(&gtk::Label::builder().label(subtitle).xalign(0.0).wrap(true).css_classes(["s2s-row-subtitle"]).build());
    }
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    vb.append(&body);
    let r = gtk::ListBoxRow::builder().child(&vb).activatable(false).build();
    list.append(&r);
    (r, body, info)
}

pub fn set_badge(l: &gtk::Label, text: &str, kind: &str) {
    for k in ["ok", "warn", "error"] {
        l.remove_css_class(k);
    }
    l.add_css_class("s2s-badge");
    if !kind.is_empty() {
        l.add_css_class(kind);
    }
    l.set_text(text);
    l.set_visible(!text.is_empty());
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// One or more files (several become a glob pattern).
    Files,
    Folder,
}

/// Pattern entry + browse button + drag-and-drop. Calls `on_pick` with the
/// new pattern text (the entry is updated too).
pub fn path_picker(
    st: &Rc<AppState>,
    parent: &impl IsA<gtk::Widget>,
    lens: Lens<String>,
    pick: Pick,
    title: &'static str,
    filter: Option<(&'static str, &'static [&'static str])>,
) -> (gtk::Box, gtk::Entry) {
    let b = gtk::Box::builder().spacing(6).hexpand(true).build();
    let e = entry(st, lens);
    e.set_placeholder_text(Some(match pick {
        Pick::Folder => "Folder",
        _ => "File, folder or pattern (e.g. ~/anime/show/*.ass)",
    }));
    let btn = gtk::Button::builder()
        .icon_name(if pick == Pick::Folder { "folder-open-symbolic" } else { "document-open-symbolic" })
        .tooltip_text("Browse…")
        .valign(gtk::Align::Center)
        .build();
    b.append(&e);
    b.append(&btn);

    let parent = parent.clone().upcast::<gtk::Widget>();
    let (s, ew) = (st.clone(), e.downgrade());
    btn.connect_clicked(move |_| {
        let Some(win) = parent.root().and_downcast::<gtk::Window>() else { return };
        let dialog = gtk::FileDialog::builder().title(title).modal(true).build();
        let start = {
            let cur = ew.upgrade().map(|e| e.text().to_string()).unwrap_or_default();
            let cur = subs2srs_core::files::expand_tilde(&cur);
            let p = PathBuf::from(&cur);
            let dir = if p.is_dir() { Some(p) } else { p.parent().map(PathBuf::from).filter(|d| d.is_dir()) };
            dir.or_else(|| {
                let l = s.prefs.borrow().last_dir.clone();
                (!l.is_empty()).then(|| PathBuf::from(l))
            })
        };
        if let Some(d) = start {
            dialog.set_initial_folder(Some(&gio::File::for_path(d)));
        }
        if let Some((name, exts)) = filter {
            let f = gtk::FileFilter::new();
            f.set_name(Some(name));
            for x in exts {
                f.add_suffix(x);
            }
            let all = gtk::FileFilter::new();
            all.set_name(Some("All files"));
            all.add_pattern("*");
            let store = gio::ListStore::new::<gtk::FileFilter>();
            store.append(&f);
            store.append(&all);
            dialog.set_filters(Some(&store));
            dialog.set_default_filter(Some(&f));
        }
        let (s2, ew2) = (s.clone(), ew.clone());
        let apply = move |paths: Vec<PathBuf>| {
            if paths.is_empty() {
                return;
            }
            if let Some(dir) = paths[0].parent() {
                s2.prefs.borrow_mut().last_dir = dir.display().to_string();
            }
            let text = if pick == Pick::Folder { paths[0].display().to_string() } else { subs2srs_core::files::pattern_for(&paths) };
            if let Some(e) = ew2.upgrade() {
                e.set_text(&text);
            }
        };
        match pick {
            Pick::Folder => dialog.select_folder(Some(&win), gio::Cancellable::NONE, move |r| {
                if let Ok(f) = r {
                    apply(f.path().into_iter().collect());
                }
            }),
            Pick::Files => dialog.open_multiple(Some(&win), gio::Cancellable::NONE, move |r| {
                if let Ok(list) = r {
                    let paths: Vec<PathBuf> = (0..list.n_items())
                        .filter_map(|i| list.item(i).and_downcast::<gio::File>())
                        .filter_map(|f| f.path())
                        .collect();
                    apply(paths);
                }
            }),
        }
    });

    // Drag files from the file manager onto the field.
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let ew = e.downgrade();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(list) = value.get::<gdk::FileList>() else { return false };
        let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
        if paths.is_empty() {
            return false;
        }
        let text = if pick == Pick::Folder {
            let p = &paths[0];
            (if p.is_dir() { p.clone() } else { p.parent().map(PathBuf::from).unwrap_or_default() }).display().to_string()
        } else if paths.len() == 1 && paths[0].is_dir() {
            paths[0].display().to_string()
        } else {
            subs2srs_core::files::pattern_for(&paths)
        };
        if let Some(e) = ew.upgrade() {
            e.set_text(&text);
        }
        true
    });
    b.add_controller(drop);
    (b, e)
}

pub fn icon_label(icon: &str, text: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.append(&gtk::Image::from_icon_name(icon));
    b.append(&gtk::Label::new(Some(text)));
    b
}

pub fn alert(parent: &impl IsA<gtk::Widget>, message: &str, detail: &str) {
    let win = parent.as_ref().root().and_downcast::<gtk::Window>();
    let d = gtk::AlertDialog::builder().message(message).detail(detail).modal(true).build();
    d.show(win.as_ref());
}

/// Ask a question; `on_answer(index)` gets the chosen button index.
pub fn confirm(parent: &impl IsA<gtk::Widget>, message: &str, detail: &str, buttons: &[&str], on_answer: impl FnOnce(i32) + 'static) {
    let win = parent.as_ref().root().and_downcast::<gtk::Window>();
    let d = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .modal(true)
        .buttons(buttons.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .cancel_button(0)
        .default_button(buttons.len() as i32 - 1)
        .build();
    d.choose(win.as_ref(), gio::Cancellable::NONE, move |r| on_answer(r.unwrap_or(0)));
}
