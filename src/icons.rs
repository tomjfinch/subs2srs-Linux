//! Icons are bundled so the UI looks the same with any icon theme.

use gtk::gdk;

const ICONS: &[(&str, &str)] = &[
    ("s2s-audio-symbolic", include_str!("../data/icons/symbolic/s2s-audio-symbolic.svg")),
    ("s2s-context-symbolic", include_str!("../data/icons/symbolic/s2s-context-symbolic.svg")),
    ("s2s-filter-symbolic", include_str!("../data/icons/symbolic/s2s-filter-symbolic.svg")),
    ("s2s-preview-symbolic", include_str!("../data/icons/symbolic/s2s-preview-symbolic.svg")),
    ("s2s-snapshot-symbolic", include_str!("../data/icons/symbolic/s2s-snapshot-symbolic.svg")),
    ("s2s-source-symbolic", include_str!("../data/icons/symbolic/s2s-source-symbolic.svg")),
    ("s2s-subtitles-symbolic", include_str!("../data/icons/symbolic/s2s-subtitles-symbolic.svg")),
    ("s2s-timing-symbolic", include_str!("../data/icons/symbolic/s2s-timing-symbolic.svg")),
    ("s2s-tools-symbolic", include_str!("../data/icons/symbolic/s2s-tools-symbolic.svg")),
    ("s2s-video-symbolic", include_str!("../data/icons/symbolic/s2s-video-symbolic.svg")),
    (crate::APP_ID, include_str!("../data/icons/app/io.github.subs2srs.Subs2srs.svg")),
];

/// Write the icons to the cache dir and add it to the icon theme search path.
pub fn install() {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("subs2srs")
        .join(concat!("icons-", env!("CARGO_PKG_VERSION")));
    // hicolor/scalable/<context>/ is listed in every hicolor index.theme, so
    // GTK finds the icons through its normal theme fallback.
    for (name, svg) in ICONS {
        let ctx = if name.ends_with("-symbolic") { "actions" } else { "apps" };
        let sub = dir.join("hicolor").join("scalable").join(ctx);
        if std::fs::create_dir_all(&sub).is_err() {
            return;
        }
        let path = sub.join(format!("{name}.svg"));
        if std::fs::read_to_string(&path).map(|s| s != *svg).unwrap_or(true) {
            let _ = std::fs::write(&path, svg);
        }
    }
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(&dir);
    }
}
