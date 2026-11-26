//! Turning a user "file pattern" into a sorted list of files.
//!
//! A pattern can be a single file, a directory (all matching files in it) or
//! a glob in the last path component (`/videos/Show - *.mkv`).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

pub const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "webm", "mov", "wmv", "flv", "ogm", "ogv", "ts", "m2ts", "mpg", "mpeg", "vob", "rmvb",
];
pub const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "aac", "flac", "ogg", "opus", "wav", "wma", "mka", "ac3"];

/// Expand `pattern`. `exts` filters directory listings and globs (lowercase,
/// without dot); an explicitly named file is always returned.
pub fn expand(pattern: &str, exts: &[&str]) -> Vec<PathBuf> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Vec::new();
    }
    let pattern = expand_tilde(pattern);
    let p = Path::new(&pattern);
    if p.is_dir() {
        return list(p, "*", exts);
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.contains(['*', '?']) {
        let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        return list(dir, name, exts);
    }
    if p.is_file() {
        vec![p.to_path_buf()]
    } else {
        Vec::new()
    }
}

/// Build a pattern that matches `files` (used when several files are picked
/// in the file chooser): `dir/*.ext` when they share a directory and extension
/// and that glob matches exactly them, otherwise the common directory.
pub fn pattern_for(files: &[PathBuf]) -> String {
    match files {
        [] => String::new(),
        [one] => one.display().to_string(),
        many => {
            let dir = many[0].parent().map(Path::to_path_buf).unwrap_or_default();
            let ext = many[0].extension().map(|e| e.to_string_lossy().to_string());
            let same_dir = many.iter().all(|f| f.parent() == Some(dir.as_path()));
            let same_ext = many.iter().all(|f| f.extension().map(|e| e.to_string_lossy().to_string()) == ext);
            if same_dir && same_ext {
                if let Some(ext) = ext {
                    let pat = dir.join(format!("*.{ext}")).display().to_string();
                    let mut a: Vec<PathBuf> = many.to_vec();
                    a.sort_by(|x, y| natural_cmp(&x.to_string_lossy(), &y.to_string_lossy()));
                    if expand(&pat, &[]) == a {
                        return pat;
                    }
                    // Find the longest common prefix of the file names to build `prefix*.ext`.
                    let names: Vec<String> =
                        a.iter().map(|f| f.file_name().unwrap().to_string_lossy().to_string()).collect();
                    let prefix = common_prefix(&names);
                    let pat2 = dir.join(format!("{prefix}*.{ext}")).display().to_string();
                    if expand(&pat2, &[]) == a {
                        return pat2;
                    }
                    return pat;
                }
            }
            dir.display().to_string()
        }
    }
}

fn common_prefix(names: &[String]) -> String {
    let mut prefix: &str = &names[0];
    for n in &names[1..] {
        let mut end = 0;
        for ((i, a), b) in prefix.char_indices().zip(n.chars()) {
            if a != b {
                break;
            }
            end = i + a.len_utf8();
        }
        prefix = &prefix[..end];
    }
    prefix.replace(['*', '?'], "")
}

pub fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest).display().to_string();
        }
    }
    p.to_string()
}

fn list(dir: &Path, glob: &str, exts: &[&str]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file() || t.is_symlink()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !name.starts_with('.') && glob_match(glob, name)
        })
        .filter(|p| {
            exts.is_empty()
                || p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| exts.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    out
}

/// `*` and `?` wildcards; case-sensitive like the filesystem.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// "Episode 2" sorts before "Episode 10".
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase()).then(x.cmp(&y));
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globbing() {
        assert!(glob_match("*.ass", "ep01.ass"));
        assert!(glob_match("Show - ??.mkv", "Show - 01.mkv"));
        assert!(!glob_match("*.ass", "ep01.srt"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn natural() {
        let mut v = vec!["ep10", "ep2", "Ep1", "ep02b"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Ep1", "ep2", "ep02b", "ep10"]);
    }

    #[test]
    fn expand_dir_and_pattern() {
        let d = std::env::temp_dir().join(format!("s2s-files-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for f in ["b 10.ass", "b 2.ass", "x.srt", ".hidden.ass"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let got = expand(d.to_str().unwrap(), &["ass", "srt"]);
        let names: Vec<_> = got.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["b 2.ass", "b 10.ass", "x.srt"]);
        let got = expand(&format!("{}/*.ass", d.display()), &[]);
        assert_eq!(got.len(), 2);
        let pat = pattern_for(&got);
        assert_eq!(pat, format!("{}/*.ass", d.display()));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
