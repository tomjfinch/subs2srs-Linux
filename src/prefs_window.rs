//! Preferences: appearance, behaviour and every naming format.

use crate::ui::{self, AppState};
use gtk::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;
use subs2srs_core::settings::{Prefs, Theme};

type PLens = for<'a> fn(&'a mut Prefs) -> &'a mut String;

struct PrefsUi {
    prefs: Rc<RefCell<Prefs>>,
    loaders: RefCell<Vec<Box<dyn Fn()>>>,
}

impl PrefsUi {
    fn entry(self: &Rc<Self>, lens: PLens) -> gtk::Entry {
        let e = gtk::Entry::builder().hexpand(true).build();
        e.set_text(lens(&mut self.prefs.borrow_mut()));
        let p = self.prefs.clone();
        e.connect_changed(move |e| *lens(&mut p.borrow_mut()) = e.text().to_string());
        let (p, w) = (self.prefs.clone(), e.downgrade());
        self.loaders.borrow_mut().push(Box::new(move || {
            if let Some(e) = w.upgrade() {
                let v = lens(&mut p.borrow_mut()).clone();
                e.set_text(&v);
            }
        }));
        e
    }

    fn format_row(self: &Rc<Self>, list: &gtk::ListBox, title: &str, sub: &str, lens: PLens) {
        let (_, body, _) = ui::wide_row(list, title, sub);
        let e = self.entry(lens);
        e.add_css_class("monospace");
        body.append(&e);
    }
}

pub fn open(parent: &gtk::ApplicationWindow, st: Rc<AppState>) {
    let win = gtk::Window::builder().title("Preferences").transient_for(parent).modal(true).default_width(760).default_height(720).build();
    let header = gtk::HeaderBar::new();
    let reset = gtk::Button::builder().label("Restore Defaults").tooltip_text("Reset all naming formats and options").build();
    header.pack_start(&reset);
    win.set_titlebar(Some(&header));

    // Start from what's on disk so concurrent windows don't clobber each other.
    let mut initial = Prefs::load();
    {
        let cur = st.prefs.borrow();
        initial.window_width = cur.window_width;
        initial.window_height = cur.window_height;
        initial.last_dir = cur.last_dir.clone();
    }
    let pu = Rc::new(PrefsUi { prefs: Rc::new(RefCell::new(initial)), loaders: RefCell::new(Vec::new()) });
    let (pg, b) = ui::page("Preferences", "Saved to ~/.config/subs2srs/preferences.json when you close this window.");

    // Appearance
    let g = ui::group(&b, "Appearance", Some("subs2srs uses your GTK theme. “Follow System” also follows the desktop's light/dark setting."));
    let theme_dd = gtk::DropDown::from_strings(&["Follow System", "Light", "Dark"]);
    theme_dd.set_valign(gtk::Align::Center);
    theme_dd.set_selected(match pu.prefs.borrow().theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    });
    let (p, par) = (pu.prefs.clone(), parent.clone());
    theme_dd.connect_selected_notify(move |dd| {
        let t = match dd.selected() {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => Theme::System,
        };
        p.borrow_mut().theme = t;
        let _ = WidgetExt::activate_action(&par, "win.theme", Some(&crate::window::theme_name(t).to_variant()));
    });
    ui::row_with(&g, "Style", "", &theme_dd);

    // Behaviour
    let g = ui::group(&b, "Behaviour", None);
    let threads = gtk::SpinButton::with_range(0.0, 64.0, 1.0);
    threads.set_valign(gtk::Align::Center);
    threads.set_value(pu.prefs.borrow().max_parallel as f64);
    let p = pu.prefs.clone();
    threads.connect_value_changed(move |s| p.borrow_mut().max_parallel = s.value() as usize);
    ui::row_with(&g, "Parallel ffmpeg jobs", &format!("0 = one per CPU thread ({} here)", Prefs::default().threads()), &threads);
    let (_, body, _) = ui::wide_row(&g, "Audio player for previews", "Leave empty to use mpv, ffplay, pw-play or paplay — whichever is installed. The clip path is appended.");
    let e = pu.entry(|p| &mut p.audio_player);
    e.set_placeholder_text(Some("e.g. mpv --no-video"));
    body.append(&e);

    // Anki file
    let g = ui::group(&b, "Anki import file", None);
    let hdr = gtk::Switch::builder().valign(gtk::Align::Center).active(pu.prefs.borrow().anki_headers).build();
    let p = pu.prefs.clone();
    hdr.connect_active_notify(move |s| p.borrow_mut().anki_headers = s.is_active());
    ui::row_with(&g, "Write Anki headers", "Adds #separator, #html and #tags column lines so Anki 2.1.55+ sets up the import for you.", &hdr);
    let delim = gtk::DropDown::from_strings(&["Tab", "Comma", "Semicolon", "Pipe"]);
    delim.set_valign(gtk::Align::Center);
    let delims = ["\t", ",", ";", "|"];
    delim.set_selected(delims.iter().position(|d| *d == pu.prefs.borrow().srs_delimiter).unwrap_or(0) as u32);
    let p = pu.prefs.clone();
    delim.connect_selected_notify(move |dd| p.borrow_mut().srs_delimiter = delims[dd.selected().min(3) as usize].to_string());
    ui::row_with(&g, "Field separator", "", &delim);
    pu.format_row(&g, "File name", "", |p| &mut p.srs_filename_format);
    pu.format_row(&g, "Tag", "First column; leave empty to omit it.", |p| &mut p.srs_tag_format);
    pu.format_row(&g, "Sequence marker", "Leave empty to omit it.", |p| &mut p.srs_sequence_marker_format);
    pu.format_row(&g, "Subs1 field", "", |p| &mut p.srs_subs1_format);
    pu.format_row(&g, "Subs2 field", "", |p| &mut p.srs_subs2_format);

    let g = ui::group(&b, "Audio clips", Some("The extension comes from the audio format."));
    pu.format_row(&g, "File name", "", |p| &mut p.audio_filename_format);
    pu.format_row(&g, "Field prefix", "", |p| &mut p.audio_prefix);
    pu.format_row(&g, "Field suffix", "", |p| &mut p.audio_suffix);
    pu.format_row(&g, "Artist tag", "", |p| &mut p.audio_tag_artist);
    pu.format_row(&g, "Album tag", "", |p| &mut p.audio_tag_album);
    pu.format_row(&g, "Title tag", "", |p| &mut p.audio_tag_title);
    pu.format_row(&g, "Genre tag", "", |p| &mut p.audio_tag_genre);
    pu.format_row(&g, "Lyrics tag", "", |p| &mut p.audio_tag_lyrics);

    let g = ui::group(&b, "Snapshots", Some("Include the extension: .jpg, .webp, .png or .avif."));
    pu.format_row(&g, "File name", "", |p| &mut p.snapshot_filename_format);
    pu.format_row(&g, "Field prefix", "", |p| &mut p.snapshot_prefix);
    pu.format_row(&g, "Field suffix", "", |p| &mut p.snapshot_suffix);

    let g = ui::group(&b, "Video clips", Some("The extension comes from the video format."));
    pu.format_row(&g, "File name", "", |p| &mut p.video_filename_format);
    pu.format_row(&g, "Field prefix", "", |p| &mut p.video_prefix);
    pu.format_row(&g, "Field suffix", "", |p| &mut p.video_suffix);

    let g = ui::group(&b, "Extract audio tool", None);
    pu.format_row(&g, "File name", "", |p| &mut p.extract_audio_filename_format);
    pu.format_row(&g, "Subs1 lyrics line", "", |p| &mut p.extract_lyrics_subs1_format);
    pu.format_row(&g, "Subs2 lyrics line", "", |p| &mut p.extract_lyrics_subs2_format);

    let g = ui::group(&b, "Dueling subtitles tool", None);
    pu.format_row(&g, "Subtitle file name", "", |p| &mut p.dueling_filename_format);
    pu.format_row(&g, "Quick reference file name", "", |p| &mut p.dueling_quickref_filename_format);
    pu.format_row(&g, "Quick reference Subs1 line", "", |p| &mut p.dueling_quickref_subs1_format);
    pu.format_row(&g, "Quick reference Subs2 line", "", |p| &mut p.dueling_quickref_subs2_format);

    // Token reference
    let g = ui::group(&b, "Format tokens", Some("${3:episode_num} pads to 3 digits; ${0:…} pads to the widest value needed."));
    let tokens = [
        ("${deck_name}", "Deck name"),
        ("${episode_num}", "Episode number"),
        ("${sequence_num}", "Running line number"),
        ("${total_line_num}", "Number of lines"),
        ("${subs1_line} ${subs2_line}", "Line text"),
        ("${s_…} ${e_…} ${d_…} ${m_…}", "Start, end, duration, middle time…"),
        ("…hour …min …sec …hsec …msec", "…as clock parts"),
        ("…total_hour …total_min …total_sec …total_msec", "…or totals"),
        ("${width} ${height}", "Video clip size"),
        ("${tab} ${cr} ${lf}", "Tab, carriage return, line feed"),
    ];
    for (tok, what) in tokens {
        let l = gtk::Label::builder().label(tok).css_classes(["s2s-token"]).selectable(true).build();
        ui::row_with(&g, what, "", &l);
    }
    win.set_child(Some(&pg));

    let pu2 = pu.clone();
    let (td, th, tl, dl) = (theme_dd.clone(), threads.clone(), hdr.clone(), delim.clone());
    reset.connect_clicked(move |_| {
        let keep = pu2.prefs.borrow().clone();
        let mut d = Prefs::default();
        d.window_width = keep.window_width;
        d.window_height = keep.window_height;
        d.last_dir = keep.last_dir;
        *pu2.prefs.borrow_mut() = d;
        for f in pu2.loaders.borrow().iter() {
            f();
        }
        td.set_selected(0);
        th.set_value(0.0);
        tl.set_active(true);
        dl.set_selected(0);
    });

    let (pu2, st2) = (pu.clone(), st.clone());
    win.connect_close_request(move |w| {
        let p = pu2.prefs.borrow().clone();
        *st2.prefs.borrow_mut() = p.clone();
        if let Err(e) = p.save() {
            if let Some(par) = w.transient_for() {
                ui::alert(&par, "Could not save preferences", &e.to_string());
            }
        }
        glib::Propagation::Proceed
    });
    win.present();
}
