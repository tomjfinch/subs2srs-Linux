//! Light/dark handling that respects the user's GTK theme.
//!
//! * The GTK theme itself (`gtk-theme-name` from settings.ini / XSettings /
//!   GSettings) is always used — we never force Adwaita.
//! * "System" follows the freedesktop colour-scheme portal
//!   (`org.freedesktop.appearance color-scheme`), which GNOME, KDE and most
//!   portals implement, and live-updates when it changes. Without a portal
//!   GTK's own `gtk-application-prefer-dark-theme` setting is left alone.
//! * "Light"/"Dark" set `gtk-application-prefer-dark-theme`. Themes that ship
//!   the dark variant as a separate theme (`Foo-dark`, `Foo-Dark`) are
//!   switched to that sibling, and back.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use subs2srs_core::settings::Theme;

thread_local! {
    static STATE: RefCell<Option<ThemeState>> = const { RefCell::new(None) };
}

struct ThemeState {
    settings: gtk::Settings,
    /// Theme name and dark preference before we touched anything.
    orig_theme: String,
    orig_prefer_dark: bool,
    /// Portal colour scheme: 0 = no preference, 1 = dark, 2 = light. None = no portal.
    portal: Cell<Option<u32>>,
    mode: Cell<Theme>,
    _proxy: RefCell<Option<gio::DBusProxy>>,
}

pub fn init() {
    let Some(settings) = gtk::Settings::default() else { return };
    let orig_theme = settings.gtk_theme_name().map(|s| s.to_string()).unwrap_or_default();
    let orig_prefer_dark = settings.is_gtk_application_prefer_dark_theme();
    STATE.with(|s| {
        *s.borrow_mut() = Some(ThemeState {
            settings,
            orig_theme,
            orig_prefer_dark,
            portal: Cell::new(None),
            mode: Cell::new(Theme::System),
            _proxy: RefCell::new(None),
        })
    });
    connect_portal();
}

/// Apply a theme mode (called at startup and when the user changes it).
pub fn set_mode(mode: Theme) {
    STATE.with(|s| {
        if let Some(st) = s.borrow().as_ref() {
            st.mode.set(mode);
            apply(st);
        }
    });
}

fn want_dark(st: &ThemeState) -> Option<bool> {
    match st.mode.get() {
        Theme::Light => Some(false),
        Theme::Dark => Some(true),
        Theme::System => match st.portal.get() {
            Some(1) => Some(true),
            Some(2) => Some(false),
            // Portal says "no preference" or there's no portal: keep the user's GTK config.
            _ => None,
        },
    }
}

fn apply(st: &ThemeState) {
    let Some(dark) = want_dark(st) else {
        if st.settings.gtk_theme_name().map(|s| s.to_string()).unwrap_or_default() != st.orig_theme {
            st.settings.set_gtk_theme_name(Some(&st.orig_theme));
        }
        st.settings.set_gtk_application_prefer_dark_theme(st.orig_prefer_dark);
        return;
    };
    let theme = pick_theme(&st.orig_theme, dark);
    if st.settings.gtk_theme_name().map(|s| s.to_string()).unwrap_or_default() != theme {
        st.settings.set_gtk_theme_name(Some(&theme));
    }
    st.settings.set_gtk_application_prefer_dark_theme(dark);
}

fn name_is_dark(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with("-dark") || l.ends_with("_dark") || l.ends_with("dark")
}

/// Choose the theme name to use for light/dark, staying in the user's theme family.
fn pick_theme(orig: &str, dark: bool) -> String {
    if orig.is_empty() {
        return orig.to_string();
    }
    let builtin = matches!(orig, "Adwaita" | "Default" | "HighContrast" | "HighContrastInverse");
    if builtin {
        return orig.to_string();
    }
    if dark {
        if name_is_dark(orig) || has_gtk4_file(orig, "gtk-dark.css") {
            return orig.to_string();
        }
        for cand in [format!("{orig}-dark"), format!("{orig}-Dark"), format!("{orig}Dark")] {
            if has_gtk4_file(&cand, "gtk.css") {
                return cand;
            }
        }
        orig.to_string()
    } else {
        if !name_is_dark(orig) {
            return orig.to_string();
        }
        let base = orig
            .strip_suffix("-dark")
            .or_else(|| orig.strip_suffix("-Dark"))
            .or_else(|| orig.strip_suffix("Dark"))
            .unwrap_or(orig);
        for cand in [base.to_string(), format!("{base}-light"), format!("{base}-Light")] {
            if cand != orig && has_gtk4_file(&cand, "gtk.css") {
                return cand;
            }
        }
        orig.to_string()
    }
}

fn theme_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(d) = data_home {
        v.push(d.join("themes"));
    }
    if let Some(h) = std::env::var_os("HOME") {
        v.push(PathBuf::from(h).join(".themes"));
    }
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d).join("themes"));
    }
    v
}

fn has_gtk4_file(theme: &str, file: &str) -> bool {
    theme_dirs().iter().any(|d| d.join(theme).join("gtk-4.0").join(file).is_file())
}

fn connect_portal() {
    gio::DBusProxy::for_bus(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
        None,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
        gio::Cancellable::NONE,
        |res| {
            let Ok(proxy) = res else { return };
            // Live updates. The closure must be Send + Sync, so it only forwards
            // the value to the main thread via thread-local state.
            proxy.connect_g_signal(|_, _, signal, params| {
                if signal != "SettingChanged" {
                    return;
                }
                if let Some((ns, key, value)) = params.get::<(String, String, glib::Variant)>() {
                    if ns == "org.freedesktop.appearance" && key == "color-scheme" {
                        if let Some(v) = unwrap_u32(&value) {
                            glib::MainContext::default().invoke(move || portal_changed(v));
                        }
                    }
                }
            });
            let args = ("org.freedesktop.appearance", "color-scheme").to_variant();
            let p2 = proxy.clone();
            proxy.call(
                "ReadOne",
                Some(&args),
                gio::DBusCallFlags::NONE,
                2000,
                gio::Cancellable::NONE,
                move |r| match r {
                    Ok(v) => {
                        if let Some(x) = v.child_value(0).as_variant().as_ref().and_then(unwrap_u32) {
                            portal_changed(x);
                        }
                    }
                    Err(_) => {
                        // Older portals only have Read(), which double-wraps the value.
                        let args = ("org.freedesktop.appearance", "color-scheme").to_variant();
                        p2.call("Read", Some(&args), gio::DBusCallFlags::NONE, 2000, gio::Cancellable::NONE, |r| {
                            if let Ok(v) = r {
                                if let Some(x) = v.child_value(0).as_variant().as_ref().and_then(unwrap_u32) {
                                    portal_changed(x);
                                }
                            }
                        });
                    }
                },
            );
            STATE.with(|s| {
                if let Some(st) = s.borrow().as_ref() {
                    *st._proxy.borrow_mut() = Some(proxy);
                }
            });
        },
    );
}

fn unwrap_u32(v: &glib::Variant) -> Option<u32> {
    if let Some(x) = v.get::<u32>() {
        return Some(x);
    }
    v.as_variant().as_ref().and_then(unwrap_u32)
}

fn portal_changed(v: u32) {
    STATE.with(|s| {
        if let Some(st) = s.borrow().as_ref() {
            st.portal.set(Some(v));
            apply(st);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_dark_theme() {
        let d = std::env::temp_dir().join(format!("s2s-theme-{}", std::process::id()));
        for t in ["Foo", "Foo-dark", "Bar"] {
            std::fs::create_dir_all(d.join(".themes").join(t).join("gtk-4.0")).unwrap();
            std::fs::write(d.join(".themes").join(t).join("gtk-4.0/gtk.css"), "").unwrap();
        }
        std::fs::write(d.join(".themes/Bar/gtk-4.0/gtk-dark.css"), "").unwrap();
        std::env::set_var("HOME", &d);
        std::env::set_var("XDG_DATA_HOME", d.join("nope"));
        std::env::set_var("XDG_DATA_DIRS", d.join("nope"));
        assert_eq!(pick_theme("Foo", true), "Foo-dark");
        assert_eq!(pick_theme("Foo-dark", false), "Foo");
        assert_eq!(pick_theme("Foo", false), "Foo");
        // Theme with a built-in dark variant stays as is.
        assert_eq!(pick_theme("Bar", true), "Bar");
        assert_eq!(pick_theme("Adwaita", true), "Adwaita");
        let _ = std::fs::remove_dir_all(&d);
    }
}
