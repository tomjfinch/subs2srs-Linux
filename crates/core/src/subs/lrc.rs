use crate::model::Line;
use crate::time::Ms;

/// Parse LRC lyrics. A line may carry several `[mm:ss.xx]` stamps. End time
/// is the next line's start; the last line lasts 10 s. Metadata lines and
/// lines containing `:`/`：`/`www.` are dropped, as in the original.
pub fn parse(text: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while rest.starts_with('[') {
            let Some(close) = rest.find(']') else { break };
            match stamp(&rest[1..close]) {
                Some(t) => stamps.push(t),
                None => break,
            }
            rest = rest[close + 1..].trim_start();
        }
        let text = super::squash_ws(rest);
        for t in stamps {
            lines.push(Line::new(t, 0, text.clone()));
        }
    }
    lines.sort_by_key(|l| l.start);
    let n = lines.len();
    for i in 0..n {
        lines[i].end = if i + 1 < n { lines[i + 1].start } else { lines[i].start + 10_000 };
    }
    lines.retain(|l| {
        !l.text.is_empty()
            && l.start != l.end
            && !l.text.contains(':')
            && !l.text.contains('：')
            && !l.text.contains("www.")
    });
    lines
}

fn stamp(s: &str) -> Option<Ms> {
    let (m, rest) = s.split_once(':')?;
    let m: i64 = m.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once('.') {
        Some((a, b)) => (a, b),
        None => (rest, "0"),
    };
    let sec: i64 = sec.trim().parse().ok()?;
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let f3: String = frac.chars().chain("000".chars()).take(3).collect();
    Some(m * 60_000 + sec * 1000 + f3.parse::<i64>().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        let l = parse("[ar:Someone]\n[00:01.00][00:10.00]la la\n[00:05.50]next\n");
        assert_eq!(l.len(), 3);
        assert_eq!((l[0].start, l[0].end), (1000, 5500));
        assert_eq!((l[2].start, l[2].end), (10_000, 20_000));
    }
}
