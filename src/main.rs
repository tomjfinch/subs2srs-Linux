//! subs2srs — GTK4 front-end.
#![allow(clippy::too_many_arguments, clippy::type_complexity, clippy::new_ret_no_self)]

mod icons;
mod job;
mod prefs_window;
mod preview;
mod theme;
mod tools_window;
mod ui;
mod window;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

pub const APP_ID: &str = "io.github.subs2srs.Subs2srs";
pub const APP_NAME: &str = "subs2srs";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|app| {
        glib::set_application_name("subs2srs");
        gtk::Window::set_default_icon_name(APP_ID);
        load_css();
        icons::install();
        theme::init();
        install_accels(app);
    });
    app.connect_activate(|app| {
        window::MainWindow::new(app, None).present();
    });
    app.connect_open(|app, files, _| {
        for f in files {
            window::MainWindow::new(app, f.path()).present();
        }
    });
    app.run()
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}

fn install_accels(app: &gtk::Application) {
    let accels: &[(&str, &[&str])] = &[
        ("win.new", &["<Control>n"]),
        ("win.open", &["<Control>o"]),
        ("win.save", &["<Control>s"]),
        ("win.save-as", &["<Control><Shift>s"]),
        ("win.preview", &["<Control>p"]),
        ("win.generate", &["<Control>Return"]),
        ("win.preferences", &["<Control>comma"]),
        ("win.close", &["<Control>w"]),
        ("app.quit", &["<Control>q"]),
    ];
    for (action, keys) in accels {
        app.set_accels_for_action(action, keys);
    }
    let quit = gio::SimpleAction::new("quit", None);
    let a = app.clone();
    quit.connect_activate(move |_, _| {
        for w in a.windows() {
            w.close();
        }
    });
    app.add_action(&quit);
}
