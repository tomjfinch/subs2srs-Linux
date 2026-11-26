use crate::model::Line;
use crate::time::{parse_hms, Ms};
use crate::{bail, Result};

/// Parse SubRip. Tolerant of missing index lines, `.` instead of `,`, missing
/// hours, CRLF and trailing junk after the end time.
pub fn parse(text: &str) -> Result<Vec<Line>> {
    let mut out = Vec::new();
    let mut cur: Option<(Ms, Ms, Vec<String>)> = None;

    let flush = |cur: &mut Option<(Ms, Ms, Vec<String>)>, out: &mut Vec<Line>| {
        if let Some((s, e, txt)) = cur.take() {
            let joined = txt.join(" ");
            let t = super::squash_ws(&super::decode_entities(&super::strip_markup(&joined)));
            if !t.is_empty() {
                out.push(Line::new(s, e, t));
            }
        }
    };

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();
        if let Some((s, e)) = l.split_once("-->") {
            flush(&mut cur, &mut out);
            let start = time(s)?;
            let end_str = e.split_whitespace().next().unwrap_or("");
            let end = time(end_str)?;
            cur = Some((start, end, Vec::new()));
        } else if l.is_empty() {
            // A blank line ends the cue, unless the next non-blank line isn't a
            // new cue (some files have blank lines inside text). We keep it
            // simple: blank line ends text collection.
            flush(&mut cur, &mut out);
        } else if let Some((_, _, txt)) = cur.as_mut() {
            // A bare number followed by a timing line is the next cue's index.
            let is_index = l.chars().all(|c| c.is_ascii_digit())
                && lines.get(i + 1).map(|n| n.contains("-->")).unwrap_or(false);
            if !is_index {
                txt.push(l.to_string());
            }
        }
        i += 1;
    }
    flush(&mut cur, &mut out);
    Ok(out)
}

fn time(s: &str) -> Result<Ms> {
    let s = s.trim();
    match parse_hms(s) {
        Some(t) => Ok(t),
        None => bail!("Incorrect SRT time format: \"{s}\""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let s = "1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i>\r\nthere\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000 X1:0\r\n42\r\n\r\n";
        let l = parse(s).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].text, "Hello there");
        assert_eq!(l[0].end, 2500);
        assert_eq!(l[1].text, "42");
    }

    #[test]
    fn missing_index_and_blank_end() {
        let s = "00:00:01.000 --> 00:00:02.000\nA\n00:00:02.000 --> 00:00:03.000\nB";
        let l = parse(s).unwrap();
        assert_eq!(l.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }
}
