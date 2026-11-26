use crate::model::Line;
use crate::time::{parse_hms, Ms};
use crate::{bail, Result};

/// Parse WebVTT. `<v Speaker>` voice tags become the line's actor.
pub fn parse(text: &str) -> Result<Vec<Line>> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    // Skip header block.
    while let Some(l) = lines.peek() {
        if l.trim().is_empty() {
            lines.next();
            break;
        }
        if l.contains("-->") {
            break;
        }
        lines.next();
    }

    let mut skip_block = false;
    let mut cur: Option<(Ms, Ms, Vec<String>)> = None;
    let mut flush = |cur: &mut Option<(Ms, Ms, Vec<String>)>| {
        if let Some((s, e, txt)) = cur.take() {
            let joined = txt.join(" ");
            let actor = voice(&joined).unwrap_or_default();
            let t = super::squash_ws(&super::decode_entities(&super::strip_markup(&joined)));
            if !t.is_empty() {
                out.push(Line { start: s, end: e, text: t, actor });
            }
        }
    };

    for raw in lines {
        let l = raw.trim();
        if l.is_empty() {
            flush(&mut cur);
            skip_block = false;
            continue;
        }
        if skip_block {
            continue;
        }
        if cur.is_none() && (l.starts_with("NOTE") || l == "STYLE" || l == "REGION") {
            skip_block = true;
            continue;
        }
        if let Some((s, e)) = l.split_once("-->") {
            flush(&mut cur);
            let end = e.split_whitespace().next().unwrap_or("");
            cur = Some((time(s)?, time(end)?, Vec::new()));
        } else if let Some((_, _, txt)) = cur.as_mut() {
            txt.push(l.to_string());
        }
        // Otherwise it's a cue identifier; ignore it.
    }
    flush(&mut cur);
    Ok(out)
}

fn voice(s: &str) -> Option<String> {
    let i = s.find("<v ")?;
    let rest = &s[i + 3..];
    let end = rest.find('>')?;
    Some(rest[..end].trim().to_string())
}

fn time(s: &str) -> Result<Ms> {
    let s = s.trim();
    match parse_hms(s) {
        Some(t) => Ok(t),
        None => bail!("Incorrect WebVTT time format: \"{s}\""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let s = "WEBVTT\nKind: captions\n\nNOTE a comment\nmore\n\ncue-1\n00:01.000 --> 00:02.000 align:start\n<v Mary>Hi <c.yellow>there</c>\n\n00:00:03.500 --> 00:00:04.000\n&amp; bye\n";
        let l = parse(s).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].text, "Hi there");
        assert_eq!(l[0].actor, "Mary");
        assert_eq!(l[0].start, 1000);
        assert_eq!(l[1].text, "& bye");
        assert_eq!(l[1].start, 3500);
    }
}
