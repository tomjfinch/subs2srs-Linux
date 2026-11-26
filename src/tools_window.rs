//! Dialogs for the extra tools.

use crate::job;
use crate::ui::{self, AppState};
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use subs2srs_core::files;
use subs2srs_core::progress::Progress;
use subs2srs_core::settings::{self, AudioFormat};
use subs2srs_core::time;
use subs2srs_core::tools::{self, AssStyle, DuelingSettings, ExtractAudioSettings};
use subs2srs_core::Error;

fn tool_window(parent: &gtk::ApplicationWindow, title: &str) -> (gtk::Window, gtk::Box, gtk::Button, gtk::Label, gtk::ProgressBar) {
    let win = gtk::Window::builder().title(title).transient_for(parent).modal(true).default_width(720).default_height(680).build();
    let header = gtk::HeaderBar::new();
    win.set_titlebar(Some(&header));
    let (pg, b) = ui::page(title, "");
    let status = gtk::Label::builder().xalign(0.0).hexpand(true).wrap(true).css_classes(["s2s-status"]).build();
    let bar = gtk::ProgressBar::builder().visible(false).build();
    let run = gtk::Button::builder().label("Create").css_classes(["suggested-action"]).build();
    run.set_width_request(110);
    let actionbar = gtk::Box::builder().spacing(10).css_classes(["s2s-actionbar"]).build();
    let sb = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).hexpand(true).valign(gtk::Align::Center).build();
    sb.append(&status);
    sb.append(&bar);
    actionbar.append(&sb);
    actionbar.append(&run);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&pg);
    root.append(&actionbar);
    win.set_child(Some(&root));
    (win, b, run, status, bar)
}

fn config_path(name: &str) -> PathBuf {
    settings::config_dir().join(name)
}

fn load_json<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    std::fs::read_to_string(config_path(name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_json<T: serde::Serialize>(name: &str, v: &T) {
    if let Ok(j) = serde_json::to_string_pretty(v) {
        let _ = settings::write_atomic(&config_path(name), &j);
    }
}

fn run_tool<T: Send + 'static>(
    win: &gtk::Window,
    run: &gtk::Button,
    status: &gtk::Label,
    bar: &gtk::ProgressBar,
    work: impl FnOnce(&Progress) -> subs2srs_core::Result<T> + Send + 'static,
    done: impl FnOnce(&gtk::Window, T) + 'static,
) -> Arc<Progress> {
    let progress = Arc::new(Progress::new());
    run.set_label("Cancel");
    run.remove_css_class("suggested-action");
    run.add_css_class("destructive-action");
    bar.set_visible(true);
    let (st1, b1) = (status.clone(), bar.clone());
    let (st2, b2, r2, w2) = (status.clone(), bar.clone(), run.clone(), win.clone());
    job::run(
        progress.clone(),
        work,
        move |s, f| {
            st1.set_text(&format!("{} {}", s.step_name, s.detail));
            match f {
                Some(f) => b1.set_fraction(f),
                None => b1.pulse(),
            }
        },
        move |r| {
            r2.set_label("Create");
            r2.remove_css_class("destructive-action");
            r2.add_css_class("suggested-action");
            b2.set_visible(false);
            match r {
                Ok(v) => {
                    st2.set_text("Done.");
                    done(&w2, v);
                }
                Err(Error::Cancelled) => st2.set_text("Cancelled."),
                Err(e) => {
                    st2.set_text("Failed.");
                    ui::alert(&w2, "Something went wrong", &e.to_string());
                }
            }
        },
    );
    progress
}

fn written_message(win: &gtk::Window, files: Vec<PathBuf>) {
    let n = files.len();
    let dir = files.first().and_then(|f| f.parent().map(PathBuf::from));
    let list: Vec<String> = files.iter().take(8).map(|f| f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).collect();
    let more = if n > 8 { format!("\n… and {} more", n - 8) } else { String::new() };
    let first = files.first().cloned();
    let w = win.clone();
    ui::confirm(win, &format!("{n} file{} created", if n == 1 { "" } else { "s" }), &format!("{}\n\n{}{more}", dir.map(|d| d.display().to_string()).unwrap_or_default(), list.join("\n")), &["Close", "Open Folder"], move |i| {
        if let (1, Some(f)) = (i, first) {
            gtk::FileLauncher::new(Some(&gio::File::for_path(f))).open_containing_folder(Some(&w), gio::Cancellable::NONE, |_| {});
        }
    });
}

// ── Dueling subtitles ──────────────────────────────────────────────────

fn rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::WHITE)
}

fn hex(c: &gdk::RGBA) -> String {
    format!("#{:02X}{:02X}{:02X}", (c.red() * 255.0).round() as u8, (c.green() * 255.0).round() as u8, (c.blue() * 255.0).round() as u8)
}

fn style_group(b: &gtk::Box, title: &str, d: &Rc<RefCell<DuelingSettings>>, pick: fn(&mut DuelingSettings) -> &mut AssStyle) {
    let g = ui::group(b, title, None);
    let s = pick(&mut d.borrow_mut()).clone();

    let mut fd = pango::FontDescription::new();
    fd.set_family(&s.font);
    fd.set_size(s.size as i32 * pango::SCALE);
    if s.bold {
        fd.set_weight(pango::Weight::Bold);
    }
    if s.italic {
        fd.set_style(pango::Style::Italic);
    }
    let font = gtk::FontDialogButton::new(Some(gtk::FontDialog::new()));
    font.set_font_desc(&fd);
    font.set_valign(gtk::Align::Center);
    let d2 = d.clone();
    font.connect_font_desc_notify(move |b| {
        if let Some(fd) = b.font_desc() {
            let mut dd = d2.borrow_mut();
            let st = pick(&mut dd);
            if let Some(f) = fd.family() {
                st.font = f.to_string();
            }
            if fd.size() > 0 {
                st.size = (fd.size() / pango::SCALE).max(1) as u32;
            }
            st.bold = fd.weight() >= pango::Weight::Semibold;
            st.italic = fd.style() != pango::Style::Normal;
        }
    });
    ui::row_with(&g, "Font", "", &font);

    let colors = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for (label, which) in [("Text", 0), ("Outline", 1)] {
        colors.append(&gtk::Label::new(Some(label)));
        let cb = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
        cb.set_rgba(&rgba(if which == 0 { &s.primary } else { &s.outline_color }));
        let d2 = d.clone();
        cb.connect_rgba_notify(move |b| {
            let mut dd = d2.borrow_mut();
            let st = pick(&mut dd);
            if which == 0 {
                st.primary = hex(&b.rgba());
            } else {
                st.outline_color = hex(&b.rgba());
            }
        });
        colors.append(&cb);
    }
    ui::row_with(&g, "Colours", "", &colors);

    let outline = gtk::SpinButton::with_range(0.0, 10.0, 1.0);
    outline.set_value(s.outline as f64);
    outline.set_valign(gtk::Align::Center);
    let d2 = d.clone();
    outline.connect_value_changed(move |sb| pick(&mut d2.borrow_mut()).outline = sb.value() as u32);
    let boxed = gtk::Switch::builder().active(s.opaque_box).valign(gtk::Align::Center).build();
    let d2 = d.clone();
    boxed.connect_active_notify(move |sw| pick(&mut d2.borrow_mut()).opaque_box = sw.is_active());
    let ob = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    ob.append(&outline);
    ob.append(&gtk::Label::new(Some("Box")));
    ob.append(&boxed);
    ui::row_with(&g, "Outline", "Width, or draw a solid box behind the text", &ob);

    let pos = gtk::DropDown::from_strings(&["Bottom", "Middle", "Top"]);
    pos.set_valign(gtk::Align::Center);
    pos.set_selected(match s.alignment {
        7..=9 => 2,
        4..=6 => 1,
        _ => 0,
    });
    let d2 = d.clone();
    pos.connect_selected_notify(move |dd| {
        pick(&mut d2.borrow_mut()).alignment = match dd.selected() {
            2 => 8,
            1 => 5,
            _ => 2,
        }
    });
    let margin = gtk::SpinButton::with_range(0.0, 300.0, 1.0);
    margin.set_value(s.margin_v as f64);
    margin.set_valign(gtk::Align::Center);
    let d2 = d.clone();
    margin.connect_value_changed(move |sb| pick(&mut d2.borrow_mut()).margin_v = sb.value() as u32);
    let pb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    pb.append(&pos);
    pb.append(&gtk::Label::new(Some("Margin")));
    pb.append(&margin);
    ui::row_with(&g, "Position", "", &pb);
}

pub fn dueling(parent: &gtk::ApplicationWindow, st: Rc<AppState>) {
    let (win, b, run, status, bar) = tool_window(parent, "Dueling Subtitles");
    let d: Rc<RefCell<DuelingSettings>> = Rc::new(RefCell::new(load_json("dueling.json")));
    let p = st.snapshot();
    let intro = format!(
        "Uses Subs1, Subs2, timing and time shift from the main window and writes one .ass file per episode to {}.",
        if p.output_dir.is_empty() { "the output folder".to_string() } else { p.output_dir.clone() }
    );
    b.append(&gtk::Label::builder().label(intro).wrap(true).xalign(0.0).css_classes(["s2s-page-desc"]).build());

    let g = ui::group(&b, "Layout", None);
    let every = gtk::SpinButton::with_range(1.0, 20.0, 1.0);
    every.set_value(d.borrow().pattern as f64);
    every.set_valign(gtk::Align::Center);
    let d2 = d.clone();
    every.connect_value_changed(move |s| d2.borrow_mut().pattern = s.value() as usize);
    ui::row_with(&g, "Show Subs2 on every Nth line", "1 = always. Higher values wean you off the translation.", &every);
    let first = gtk::Switch::builder().active(d.borrow().subs2_first).valign(gtk::Align::Center).build();
    let d2 = d.clone();
    first.connect_active_notify(move |s| d2.borrow_mut().subs2_first = s.is_active());
    ui::row_with(&g, "Put Subs2 first", "Order of the events in the file (affects stacking).", &first);
    let qr = gtk::Switch::builder().active(d.borrow().quick_reference).valign(gtk::Align::Center).build();
    let d2 = d.clone();
    qr.connect_active_notify(move |s| d2.borrow_mut().quick_reference = s.is_active());
    ui::row_with(&g, "Also write a quick reference", "A .txt transcript with timestamps for every episode.", &qr);

    style_group(&b, "Subs1 style", &d, |d| &mut d.style1);
    style_group(&b, "Subs2 style", &d, |d| &mut d.style2);

    let running: Rc<RefCell<Option<Arc<Progress>>>> = Rc::new(RefCell::new(None));
    let (w, s, bar2, status2, d2, st2) = (win.clone(), status.clone(), bar.clone(), status.clone(), d.clone(), st.clone());
    let r = running.clone();
    run.connect_clicked(move |btn| {
        if let Some(p) = r.borrow_mut().take() {
            p.cancel();
            return;
        }
        let project = st2.snapshot();
        let prefs = st2.prefs.borrow().clone();
        let ds = d2.borrow().clone();
        save_json("dueling.json", &ds);
        let r2 = r.clone();
        let p = run_tool(
            &w,
            btn,
            &s,
            &bar2,
            move |p| tools::dueling(&project, &prefs, &ds, p),
            move |win, files| {
                r2.borrow_mut().take();
                written_message(win, files)
            },
        );
        *r.borrow_mut() = Some(p);
        let _ = &status2;
    });
    let d3 = d.clone();
    win.connect_close_request(move |_| {
        save_json("dueling.json", &*d3.borrow());
        glib::Propagation::Proceed
    });
    win.present();
}

// ── Extract audio ──────────────────────────────────────────────────────

pub fn extract_audio(parent: &gtk::ApplicationWindow, st: Rc<AppState>) {
    let (win, b, run, status, bar) = tool_window(parent, "Extract Audio");
    let p = st.snapshot();
    let mut x: ExtractAudioSettings = load_json("extract-audio.json");
    // Default to the project's inputs.
    if !p.video_pattern.is_empty() {
        x.media_pattern = p.video_pattern.clone();
        x.audio_stream = p.audio_stream;
    }
    if !p.deck_name.is_empty() {
        x.name = p.clean_deck_name();
    }
    if !p.output_dir.is_empty() {
        x.output_dir = PathBuf::from(&p.output_dir).join(format!("{}_audio", p.clean_deck_name())).display().to_string();
    }
    x.first_episode = p.first_episode;
    x.lyrics_subs1 = String::new();
    x.lyrics_subs2 = String::new();
    let x = Rc::new(RefCell::new(x));

    b.append(&gtk::Label::builder().label("Make listening files from whole episodes or fixed-length chunks.").wrap(true).xalign(0.0).css_classes(["s2s-page-desc"]).build());

    fn text_row(list: &gtk::ListBox, title: &str, sub: &str, x: &Rc<RefCell<ExtractAudioSettings>>, f: fn(&mut ExtractAudioSettings) -> &mut String) -> gtk::Entry {
        let (_, body, _) = ui::wide_row(list, title, sub);
        let e = gtk::Entry::builder().hexpand(true).text(f(&mut x.borrow_mut()).as_str()).build();
        let x2 = x.clone();
        e.connect_changed(move |e| *f(&mut x2.borrow_mut()) = e.text().to_string());
        body.append(&e);
        e
    }

    let g = ui::group(&b, "Input", None);
    let media = text_row(&g, "Media files", "Videos or audio files; defaults to the project's videos.", &x, |x| &mut x.media_pattern);
    media.set_placeholder_text(Some("~/videos/show/*.mkv"));
    let stream = gtk::SpinButton::with_range(1.0, 32.0, 1.0);
    stream.set_value(x.borrow().audio_stream as f64 + 1.0);
    stream.set_valign(gtk::Align::Center);
    let x2 = x.clone();
    stream.connect_value_changed(move |s| x2.borrow_mut().audio_stream = (s.value() as u32).saturating_sub(1));
    ui::row_with(&g, "Audio track number", "1 = first audio track", &stream);

    let g = ui::group(&b, "Output", None);
    text_row(&g, "Output folder", "", &x, |x| &mut x.output_dir);
    text_row(&g, "Name", "Used in file names and tags.", &x, |x| &mut x.name);
    let ep = gtk::SpinButton::with_range(0.0, 99999.0, 1.0);
    ep.set_value(x.borrow().first_episode as f64);
    ep.set_valign(gtk::Align::Center);
    let x2 = x.clone();
    ep.connect_value_changed(move |s| x2.borrow_mut().first_episode = s.value() as i64);
    ui::row_with(&g, "First episode number", "", &ep);
    let fmt = gtk::DropDown::from_strings(&AudioFormat::ALL.iter().map(|f| f.label()).collect::<Vec<_>>());
    fmt.set_selected(AudioFormat::ALL.iter().position(|f| *f == x.borrow().format).unwrap_or(0) as u32);
    fmt.set_valign(gtk::Align::Center);
    let x2 = x.clone();
    fmt.connect_selected_notify(move |dd| x2.borrow_mut().format = AudioFormat::ALL[dd.selected().min(2) as usize]);
    ui::row_with(&g, "Format", "", &fmt);
    let rates = [64u32, 96, 128, 160, 192, 256, 320];
    let rate = gtk::DropDown::from_strings(&rates.iter().map(|r| format!("{r} kb/s")).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>());
    rate.set_selected(rates.iter().position(|r| *r == x.borrow().bitrate).unwrap_or(2) as u32);
    rate.set_valign(gtk::Align::Center);
    let x2 = x.clone();
    rate.connect_selected_notify(move |dd| x2.borrow_mut().bitrate = rates[(dd.selected() as usize).min(rates.len() - 1)]);
    ui::row_with(&g, "Bitrate", "", &rate);

    let g = ui::group(&b, "Splitting", None);
    let chunk_sw = gtk::Switch::builder().active(x.borrow().clip_length_ms > 0).valign(gtk::Align::Center).build();
    let chunk_len = gtk::Entry::builder().width_chars(9).valign(gtk::Align::Center).text(time::to_hms(if x.borrow().clip_length_ms > 0 { x.borrow().clip_length_ms } else { 300_000 })).build();
    let cb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    cb.append(&chunk_len);
    cb.append(&chunk_sw);
    ui::row_with(&g, "Split into chunks", "Chunk length (h:mm:ss). Off = one file per episode.", &cb);
    let apply_chunk = {
        let (x2, sw, en) = (x.clone(), chunk_sw.clone(), chunk_len.clone());
        move || {
            let len = time::parse_hms(&en.text());
            if len.is_none() {
                en.add_css_class("s2s-invalid");
            } else {
                en.remove_css_class("s2s-invalid");
            }
            x2.borrow_mut().clip_length_ms = if sw.is_active() { len.unwrap_or(300_000).max(1000) } else { 0 };
            en.set_sensitive(sw.is_active());
        }
    };
    apply_chunk();
    let a1 = apply_chunk.clone();
    chunk_sw.connect_active_notify(move |_| a1());
    let a2 = apply_chunk.clone();
    chunk_len.connect_changed(move |_| a2());

    let span_sw = gtk::Switch::builder().active(x.borrow().span.enabled).valign(gtk::Align::Center).build();
    let (a, bb) = x.borrow().span.value;
    let span_a = gtk::Entry::builder().width_chars(9).text(time::to_hms(a)).valign(gtk::Align::Center).build();
    let span_b = gtk::Entry::builder().width_chars(9).text(time::to_hms(bb)).valign(gtk::Align::Center).build();
    let sbx = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    sbx.append(&span_a);
    sbx.append(&gtk::Label::new(Some("to")));
    sbx.append(&span_b);
    sbx.append(&span_sw);
    ui::row_with(&g, "Only part of each episode", "h:mm:ss", &sbx);
    let apply_span = {
        let (x2, sw, ea, eb) = (x.clone(), span_sw.clone(), span_a.clone(), span_b.clone());
        move || {
            let mut xx = x2.borrow_mut();
            xx.span.enabled = sw.is_active();
            for (e, i) in [(&ea, 0), (&eb, 1)] {
                match time::parse_hms(&e.text()) {
                    Some(v) => {
                        e.remove_css_class("s2s-invalid");
                        if i == 0 {
                            xx.span.value.0 = v
                        } else {
                            xx.span.value.1 = v
                        }
                    }
                    None => e.add_css_class("s2s-invalid"),
                }
            }
        }
    };
    for w in [&span_a, &span_b] {
        let f = apply_span.clone();
        w.connect_changed(move |_| f());
    }
    let f = apply_span.clone();
    span_sw.connect_active_notify(move |_| f());

    let g = ui::group(&b, "Lyrics", Some("Write the subtitle lines in each chunk into its lyrics tag, so music players can show them."));
    let lyr = gtk::Switch::builder().valign(gtk::Align::Center).build();
    let has_subs = !p.subs1.pattern.is_empty();
    lyr.set_sensitive(has_subs);
    ui::row_with(&g, "Use the project's subtitles as lyrics", if has_subs { "" } else { "Choose Subs1 on the Source page first." }, &lyr);
    let (x2, s1, s2, enc) = (x.clone(), p.subs1.pattern.clone(), p.subs2.pattern.clone(), p.subs1.encoding.clone());
    lyr.connect_active_notify(move |sw| {
        let mut xx = x2.borrow_mut();
        if sw.is_active() {
            xx.lyrics_subs1 = s1.clone();
            xx.lyrics_subs2 = s2.clone();
            xx.lyrics_encoding = enc.clone();
        } else {
            xx.lyrics_subs1.clear();
            xx.lyrics_subs2.clear();
        }
    });

    let count = gtk::Label::builder().xalign(0.0).css_classes(["s2s-info"]).build();
    b.append(&count);
    let x2 = x.clone();
    let update_count = move || {
        let n = files::expand(&x2.borrow().media_pattern, &[files::VIDEO_EXTS, files::AUDIO_EXTS].concat()).len();
        count.set_text(&format!("{n} media file{} found", if n == 1 { "" } else { "s" }));
    };
    update_count();
    media.connect_changed(move |_| update_count());

    let running: Rc<RefCell<Option<Arc<Progress>>>> = Rc::new(RefCell::new(None));
    let (w, s, bar2, x3, st2, r) = (win.clone(), status.clone(), bar.clone(), x.clone(), st.clone(), running.clone());
    run.connect_clicked(move |btn| {
        if let Some(p) = r.borrow_mut().take() {
            p.cancel();
            return;
        }
        let xs = x3.borrow().clone();
        let prefs = st2.prefs.borrow().clone();
        save_json("extract-audio.json", &xs);
        let r2 = r.clone();
        let p = run_tool(&w, btn, &s, &bar2, move |p| tools::extract_audio(&xs, &prefs, p), move |win, files| {
            r2.borrow_mut().take();
            written_message(win, files)
        });
        *r.borrow_mut() = Some(p);
    });
    win.present();
}
