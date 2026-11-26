//! Time values are plain milliseconds (`i64`).

pub type Ms = i64;

/// Pad (or shift) a time, clamping at zero like the original `applyTimePad`.
pub fn pad(t: Ms, pad: i64) -> Ms {
    (t + pad).max(0)
}

pub fn midpoint(start: Ms, end: Ms) -> Ms {
    start + (end - start) / 2
}

pub fn duration(start: Ms, end: Ms) -> Ms {
    (end - start).max(0)
}

/// Fraction of line 1 that overlaps line 2 (can be negative when they don't
/// overlap; the more negative the further apart they are).
pub fn overlap(s1: Ms, e1: Ms, s2: Ms, e2: Ms) -> f64 {
    let len = (e1 - s1) as f64;
    if len <= 0.0 {
        return f64::MIN;
    }
    (e1.min(e2) - s1.max(s2)) as f64 / len
}

/// `h:mm:ss`
pub fn to_hms(t: Ms) -> String {
    let s = t / 1000;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// `h:mm:ss.mmm`, used in lists.
pub fn to_hms_ms(t: Ms) -> String {
    format!("{}.{:03}", to_hms(t), t % 1000)
}

/// Seconds with millisecond precision, for ffmpeg `-ss`/`-t`.
pub fn to_ffmpeg(t: Ms) -> String {
    let t = t.max(0);
    format!("{}.{:03}", t / 1000, t % 1000)
}

/// ASS time: `h:mm:ss.cc`
pub fn to_ass(t: Ms) -> String {
    let t = t.max(0);
    format!("{}.{:02}", to_hms(t), (t % 1000) / 10)
}

/// Parse `h:mm:ss`, `mm:ss`, `ss`, each optionally followed by `.fff`.
pub fn parse_hms(s: &str) -> Option<Ms> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (main, frac) = match s.split_once(['.', ',']) {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let parts: Vec<&str> = main.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut total: i64 = 0;
    for p in &parts {
        let v: i64 = p.trim().parse().ok()?;
        if v < 0 {
            return None;
        }
        total = total * 60 + v;
    }
    let mut ms = total * 1000;
    if let Some(f) = frac {
        if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let f3: String = f.chars().chain("000".chars()).take(3).collect();
        ms += f3.parse::<i64>().ok()?;
    }
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        assert_eq!(parse_hms("1:30"), Some(90_000));
        assert_eq!(parse_hms("0:22:30"), Some(1_350_000));
        assert_eq!(parse_hms("1:02:03.5"), Some(3_723_500));
        assert_eq!(parse_hms("x"), None);
    }

    #[test]
    fn fmt() {
        assert_eq!(to_hms(3_723_500), "1:02:03");
        assert_eq!(to_ass(3_723_456), "1:02:03.45");
        assert_eq!(to_ffmpeg(1_234), "1.234");
    }

    #[test]
    fn overlap_fraction() {
        assert!((overlap(0, 1000, 500, 1500) - 0.5).abs() < 1e-9);
        assert!(overlap(0, 1000, 2000, 3000) < 0.0);
    }
}
