use super::ParseOptions;
use crate::model::Line;
use crate::time::Ms;
use crate::{bail, Result};

/// Parse an ASS/SSA script. Honours the `[Events]` `Format:` line so column
/// order doesn't matter.
pub fn parse(text: &str, opts: ParseOptions) -> Result<Vec<Line>> {
    let mut in_events = false;
    let mut cols: Vec<String> = Vec::new();
    let mut out = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(rest) = strip_prefix_ci(line, "format:") {
            cols = rest.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
            continue;
        }
        let Some(rest) = strip_prefix_ci(line, "dialogue:") else { continue };
        if cols.is_empty() {
            // SSA/ASS default when the Format line is missing.
            cols = "layer,start,end,style,name,marginl,marginr,marginv,effect,text"
                .split(',')
                .map(String::from)
                .collect();
        }
        let fields: Vec<&str> = rest.splitn(cols.len(), ',').collect();
        if fields.len() < cols.len() {
            continue;
        }
        let get = |name: &str| -> Option<&str> {
            cols.iter().position(|c| c == name).map(|i| fields[i].trim())
        };
        let (Some(start), Some(end)) = (get("start"), get("end")) else { continue };
        let actor = get("name").or_else(|| get("actor")).unwrap_or("").to_string();
        let raw_text = cols
            .iter()
            .position(|c| c == "text")
            .map(|i| fields[i].trim())
            .unwrap_or("");

        if opts.remove_styled && is_sign_or_karaoke(raw_text) {
            continue;
        }
        let mut t = raw_text.replace("\\h", " ");
        if !opts.keep_line_breaks {
            t = t.replace("\\N", " ").replace("\\n", " ");
        }
        let t = strip_overrides(&t);
        let t = if opts.keep_line_breaks {
            t.split("\\N").map(super::squash_ws).collect::<Vec<_>>().join("\\N")
        } else {
            super::squash_ws(&t)
        };
        if t.is_empty() {
            continue;
        }
        let start = parse_time(start)?;
        let end = parse_time(end)?;
        out.push(Line { start, end, text: t, actor });
    }
    Ok(out)
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Does the line start with an override block that positions, draws or
/// times karaoke? Those are signs/songs rather than dialogue. Plain styling
/// at the start of a line (`{\\i1}`, `{\\c&H..&}`, `{\\fad(..)}`) is kept.
/// (The original dropped every line starting with `{`.)
pub(crate) fn is_sign_or_karaoke(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('{') else { return false };
    let Some(end) = rest.find('}') else { return false };
    let block = &rest[..end];
    block.split('\\').skip(1).any(|tag| {
        let t = tag.trim();
        let name: String = t.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let arg = &t[name.len()..];
        match name.as_str() {
            "pos" | "move" | "org" | "clip" | "iclip" | "k" | "K" | "kf" | "ko" | "fr" | "frx" | "fry" | "frz" => true,
            "p" => arg.trim_start().starts_with(|c: char| c.is_ascii_digit() && c != '0'),
            _ => false,
        }
    })
}

fn strip_overrides(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// `h:mm:ss.cc` (hours may have more than one digit; fraction may be 2 or 3 digits).
pub(crate) fn parse_time(s: &str) -> Result<Ms> {
    match crate::time::parse_hms(s) {
        Some(t) => Ok(t),
        None => bail!(
            "Incorrect time format: \"{s}\". Re-saving the file in Aegisub usually fixes this."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Script Info]\nTitle: x\n\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.50,Default,Taro,0,0,0,,{\\i1}Hello,\\Nworld{\\i0}\nDialogue: 0,0:00:03.00,0:00:04.00,Sign,,0,0,0,,{\\pos(1,1)}SIGN\nComment: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,ignored\n";

    #[test]
    fn basic() {
        let l = parse(SAMPLE, ParseOptions::default()).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].start, 1000);
        assert_eq!(l[0].end, 2500);
        assert_eq!(l[0].text, "Hello, world");
        assert_eq!(l[0].actor, "Taro");
    }

    #[test]
    fn styled_removed() {
        let opts = ParseOptions { remove_styled: true, ..Default::default() };
        let l = parse(SAMPLE, opts).unwrap();
        assert_eq!(l.len(), 1, "the \\pos sign is removed, the italic line is kept");
        assert_eq!(l[0].text, "Hello, world");
        assert!(is_sign_or_karaoke("{\\an8\\pos(1,2)}x"));
        assert!(is_sign_or_karaoke("{\\k20}ka{\\k30}ra"));
        assert!(is_sign_or_karaoke("{\\p1}m 0 0 l 1 1"));
        assert!(!is_sign_or_karaoke("{\\i1}x"));
        assert!(!is_sign_or_karaoke("{\\fad(200,200)}x"));
        assert!(!is_sign_or_karaoke("plain"));
    }

    #[test]
    fn keep_breaks() {
        let opts = ParseOptions { keep_line_breaks: true, ..Default::default() };
        let l = parse(SAMPLE, opts).unwrap();
        assert_eq!(l[0].text, "Hello,\\Nworld");
    }
}
