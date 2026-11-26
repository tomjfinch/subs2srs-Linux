//! Subtitle parsers. All parsers work on already-decoded text.

mod ass;
mod lrc;
mod srt;
mod vtt;

use crate::model::Line;
use crate::{bail, Result};
use std::path::Path;

#[derive(Debug, Clone, Copy, Default)]
pub struct ParseOptions {
    /// Drop ASS lines whose text begins with an override block (`{...}`),
    /// which are usually signs/karaoke rather than dialogue.
    pub remove_styled: bool,
    /// Keep `\N` line breaks (used for dueling subtitles).
    pub keep_line_breaks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Ass,
    Srt,
    Vtt,
    Lrc,
}

pub const SUBTITLE_EXTS: &[&str] = &["ass", "ssa", "srt", "vtt", "lrc"];
/// Containers we can pull an embedded text subtitle track out of with ffmpeg.
pub const CONTAINER_EXTS: &[&str] = &["mkv", "mka", "mks", "mp4", "m4v", "webm", "mov"];

pub fn format_for(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "ass" | "ssa" => Format::Ass,
        "srt" => Format::Srt,
        "vtt" => Format::Vtt,
        "lrc" => Format::Lrc,
        _ => return None,
    })
}

pub fn is_container(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| CONTAINER_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn parse(text: &str, format: Format, opts: ParseOptions) -> Result<Vec<Line>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = match format {
        Format::Ass => ass::parse(text, opts)?,
        Format::Srt => srt::parse(text)?,
        Format::Vtt => vtt::parse(text)?,
        Format::Lrc => lrc::parse(text),
    };
    lines.sort_by_key(|l| l.start);
    Ok(lines)
}

/// Read + decode + parse a subtitle file.
pub fn parse_file(path: &Path, encoding: &str, opts: ParseOptions) -> Result<Vec<Line>> {
    let Some(format) = format_for(path) else {
        bail!("Unsupported subtitle format: {}", path.display());
    };
    let bytes = std::fs::read(path)
        .map_err(|e| crate::Error::msg(format!("Cannot read {}: {e}", path.display())))?;
    let text = crate::encoding::decode(&bytes, encoding);
    parse(&text, format, opts).map_err(|e| crate::Error::msg(format!("{}: {e}", path.display())))
}

/// Remove HTML-ish tags (`<i>`, `<font color=..>`, `</b>` …) and ASS override
/// blocks (`{\an8}`) that sometimes end up in SRT/VTT files.
pub(crate) fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                // Only treat as a tag if it closes on this line and looks like one.
                let rest: String = chars.clone().take_while(|&c| c != '>').collect();
                let looks_like_tag = chars.clone().any(|c| c == '>')
                    && rest
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_alphabetic() || c == '/' || c.is_ascii_digit())
                        .unwrap_or(false);
                if looks_like_tag {
                    for c in chars.by_ref() {
                        if c == '>' {
                            break;
                        }
                    }
                } else {
                    out.push(c);
                }
            }
            '{' => {
                if chars.clone().any(|c| c == '}') {
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                    }
                } else {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Collapse whitespace runs (incl. tabs/newlines) into single spaces.
pub(crate) fn squash_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode the handful of HTML entities that show up in subtitles.
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&lrm;", "")
        .replace("&rlm;", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup() {
        assert_eq!(strip_markup("<i>Hello</i> {\\an8}world"), "Hello world");
        assert_eq!(strip_markup("a < b"), "a < b");
        assert_eq!(strip_markup("<font color=\"#fff\">x</font>"), "x");
    }
}
