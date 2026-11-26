//! `${token}` name templates, compatible with the original subs2srs.
//!
//! Number tokens take an optional zero-padding width: `${3:episode_num}` →
//! `007`. A width of `0` means "as many digits as the largest value needs".
//! Time tokens are prefixed with `s_` (start), `e_` (end), `d_` (duration) or
//! `m_` (midpoint) and suffixed with `hour|min|sec|hsec|msec`, optionally with
//! `total_` (e.g. `s_total_min` = minutes since 0:00).
//! Text tokens: `${deck_name}`, `${subs1_line}`, `${subs2_line}`, `${width}`,
//! `${height}`, `${cr}`, `${lf}`, `${tab}`.

use crate::time::{self, Ms};

#[derive(Debug, Clone)]
pub struct Namer {
    pub deck_name: String,
    pub total_episodes: usize,
    pub total_lines: usize,
    /// Latest end time among all lines (decides the width of `${0:..}` time tokens).
    pub last_time: Ms,
    pub width: i64,
    pub height: i64,
    pub vobsub_stream: i64,
}

impl Namer {
    pub fn new(deck_name: &str, total_episodes: usize, total_lines: usize, last_time: Ms) -> Self {
        Namer {
            deck_name: deck_name.to_string(),
            total_episodes,
            total_lines,
            last_time,
            width: -1,
            height: -1,
            vobsub_stream: 0,
        }
    }

    pub fn with_size(mut self, width: i64, height: i64) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn create(
        &self,
        format: &str,
        episode: i64,
        sequence: i64,
        start: Ms,
        end: Ms,
        subs1: &str,
        subs2: &str,
    ) -> String {
        let mut out = String::with_capacity(format.len() + 32);
        let mut rest = format;
        while let Some(i) = rest.find("${") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 2..];
            if let Some(close) = after.find('}') {
                let inner = &after[..close];
                if let Some(v) = self.number_token(inner, episode, sequence, start, end) {
                    out.push_str(&v);
                    rest = &after[close + 1..];
                    continue;
                }
            }
            // Not a number token: keep literally, text tokens are handled below.
            out.push_str("${");
            rest = after;
        }
        out.push_str(rest);

        if self.width > -1 {
            out = out.replace("${width}", &self.width.to_string());
            out = out.replace("${height}", &self.height.to_string());
        }
        out.replace("${deck_name}", &self.deck_name)
            .replace("${subs1_line}", subs1)
            .replace("${subs2_line}", subs2)
            .replace("${cr}", "\r")
            .replace("${lf}", "\n")
            .replace("${tab}", "\t")
    }

    fn number_token(&self, inner: &str, episode: i64, sequence: i64, start: Ms, end: Ms) -> Option<String> {
        let (zeros, token) = match inner.split_once(':') {
            Some((z, t)) if z.chars().all(|c| c.is_ascii_digit()) => {
                (Some(if z.is_empty() { 0 } else { z.parse::<usize>().ok()? }), t)
            }
            Some(_) => return None,
            None => (None, inner),
        };

        let value = self.value(token, episode, sequence, start, end)?;
        let width = match zeros {
            None => 0,
            Some(0) => self.auto_width(token),
            Some(n) => n,
        }
        .min(9);
        Some(format!("{value:0width$}"))
    }

    fn value(&self, token: &str, episode: i64, sequence: i64, start: Ms, end: Ms) -> Option<i64> {
        match token {
            "episode_num" => return Some(episode),
            "sequence_num" => return Some(sequence),
            "total_line_num" => return Some(self.total_lines as i64),
            "vobsub_stream_num" => return Some(self.vobsub_stream),
            _ => {}
        }
        let (which, unit) = token.split_once('_')?;
        let t = match which {
            "s" => start,
            "e" => end,
            "d" => time::duration(start, end),
            "m" => time::midpoint(start, end),
            _ => return None,
        };
        time_part(t, unit)
    }

    fn auto_width(&self, token: &str) -> usize {
        let digits = |n: i64| n.to_string().len();
        let lt = self.last_time;
        match token {
            "episode_num" => digits(self.total_episodes as i64),
            "sequence_num" | "total_line_num" => digits(self.total_lines as i64),
            "vobsub_stream_num" => 1,
            _ => {
                let unit = token.split_once('_').map(|(_, u)| u).unwrap_or("");
                match unit {
                    "hour" => 1,
                    // Original quirk: width of the minutes *component* of the last time.
                    "min" => digits((lt / 60_000) % 60),
                    "sec" | "hsec" => 2,
                    "msec" => 3,
                    "total_hour" => digits(lt / 3_600_000),
                    "total_min" => digits(lt / 60_000),
                    "total_sec" => digits(lt / 1000),
                    "total_hsec" => digits(lt / 10),
                    "total_msec" => digits(lt),
                    _ => 0,
                }
            }
        }
    }
}

fn time_part(t: Ms, unit: &str) -> Option<i64> {
    Some(match unit {
        "hour" => (t / 3_600_000) % 24,
        "min" => (t / 60_000) % 60,
        "sec" => (t / 1000) % 60,
        "hsec" => (t % 1000) / 10,
        "msec" => t % 1000,
        "total_hour" => t / 3_600_000,
        "total_min" => t / 60_000,
        "total_sec" => t / 1000,
        "total_hsec" => t / 10,
        "total_msec" => t,
        _ => return None,
    })
}

/// Make a generated name safe to use as a file name (the original relied on
/// the defaults never containing `/`).
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | '\n' | '\r' | '\t' => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_audio_name() {
        let n = Namer::new("Show", 12, 3000, 23 * 60_000 + 40_000);
        let f = "${deck_name}_${0:episode_num}_${0:s_total_hour}.${0:s_min}.${0:s_sec}.${0:s_msec}-${0:e_total_hour}.${0:e_min}.${0:e_sec}.${0:e_msec}";
        let s = n.create(f, 3, 1, 61_234, 65_000, "", "");
        assert_eq!(s, "Show_03_0.01.01.234-0.01.05.000");
    }

    #[test]
    fn padding_and_text() {
        let n = Namer::new("D", 1, 5, 0);
        assert_eq!(n.create("${3:episode_num}${tab}${subs1_line}", 7, 0, 0, 0, "a", "b"), "007\ta");
        assert_eq!(n.create("${episode_num}", 7, 0, 0, 0, "", ""), "7");
        assert_eq!(n.create("${unknown} $5 ${", 1, 0, 0, 0, "", ""), "${unknown} $5 ${");
        assert_eq!(n.create("${2:s_total_min}:${2:s_sec}.${2:s_hsec}", 1, 0, 125_670, 0, "", ""), "02:05.67");
    }

    #[test]
    fn size_tokens() {
        let n = Namer::new("D", 1, 1, 0).with_size(480, 270);
        assert_eq!(n.create("${width}x${height}", 1, 1, 0, 0, "", ""), "480x270");
        let n = Namer::new("D", 1, 1, 0);
        assert_eq!(n.create("${width}", 1, 1, 0, 0, "", ""), "${width}");
    }
}
