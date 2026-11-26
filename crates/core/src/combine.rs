//! Matching Subs1 lines with Subs2 lines, and the line filters.
//! Ported from `WorkerSubs.cs`; behaviour is kept unless noted.

use crate::model::{Combined, Episode, Line};
use crate::settings::{Project, SubsSettings, Which};
use crate::time;
use std::collections::{HashMap, HashSet};

/// Drop lines whose end isn't after their start.
pub fn remove_bad_timing(lines: Vec<Line>) -> Vec<Line> {
    lines.into_iter().filter(|l| l.start < l.end).collect()
}

/// Join lines ending in one of `chars` (e.g. `,`, `、`, `→`) with the following
/// lines until a line ends with something else.
pub fn join_sentences(lines: Vec<Line>, chars: &str) -> Vec<Line> {
    if chars.is_empty() || lines.is_empty() {
        return lines;
    }
    let continues = |l: &Line| l.text.chars().last().map(|c| chars.contains(c)).unwrap_or(false);
    let mut out = Vec::with_capacity(lines.len());
    let mut iter = lines.into_iter().filter(|l| !l.text.is_empty()).peekable();
    while let Some(mut cur) = iter.next() {
        if continues(&cur) {
            while let Some(next) = iter.next() {
                cur.text.push(' ');
                cur.text.push_str(&next.text);
                cur.end = next.end;
                if !continues(&next) {
                    break;
                }
            }
        }
        out.push(cur);
    }
    out
}

pub fn shift(lines: &mut [Line], ms: i64) {
    if ms == 0 {
        return;
    }
    for l in lines {
        l.start = time::pad(l.start, ms);
        l.end = time::pad(l.end, ms);
    }
}

/// Prepare one parsed subtitle track: fix timing, join sentences, shift.
pub fn prepare_track(lines: Vec<Line>, s: &SubsSettings, shift_enabled: bool, episode: i64) -> Vec<Line> {
    let mut lines = remove_bad_timing(lines);
    if let Some(chars) = s.join_sentences.get() {
        lines = join_sentences(lines, chars);
    }
    if shift_enabled {
        shift(&mut lines, s.effective_shift(episode));
    }
    lines
}

/// Pair every Subs1 line with the Subs2 line that overlaps it most.
/// `subs2 = None` means there's no second subtitle (each line pairs with itself).
pub fn combine(subs1: Vec<Line>, subs2: Option<Vec<Line>>, p: &Project) -> Vec<Combined> {
    let has_subs2 = subs2.is_some();
    let subs2 = subs2.unwrap_or_else(|| subs1.clone());

    // Pass 1: best overlap for every subs1 line.
    let mut combs = Vec::with_capacity(subs1.len());
    for l1 in subs1 {
        let mut best: Option<&Line> = None;
        let mut best_overlap = f64::MIN;
        let mut stop = 0;
        for l2 in &subs2 {
            let o = time::overlap(l1.start, l1.end, l2.start, l2.end);
            if o >= best_overlap {
                best = Some(l2);
                best_overlap = o;
                stop = 0;
            } else {
                stop += 1;
                if stop == 20 {
                    break;
                }
            }
        }
        if p.subs1.remove_no_counterpart && best_overlap < 0.0 {
            continue;
        }
        let l2 = best.cloned().unwrap_or_default();
        combs.push(Combined::new(l1, l2));
    }

    // Pass 2: fix mismatches.
    if has_subs2 {
        combs = combine_consecutive_repeats(combs);
        combs = combine_skipped(&subs2, combs, p.subs2.remove_no_counterpart);
    }

    if p.timings_from == Which::Subs2 {
        for c in &mut combs {
            c.subs1.start = c.subs2.start;
            c.subs1.end = c.subs2.end;
        }
    }
    combs
}

/// Several Subs1 lines matched the same Subs2 line: merge those Subs1 lines.
fn combine_consecutive_repeats(combs: Vec<Combined>) -> Vec<Combined> {
    let mut out: Vec<Combined> = Vec::with_capacity(combs.len());
    for c in combs {
        if let Some(prev) = out.last_mut() {
            if prev.subs2.text == c.subs2.text && prev.subs2.start == c.subs2.start {
                prev.subs1.text.push(' ');
                prev.subs1.text.push_str(&c.subs1.text);
                prev.subs1.end = c.subs1.end;
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Subs2 lines nobody matched get appended to the pair they overlap most.
fn combine_skipped(subs2: &[Line], mut combs: Vec<Combined>, remove_no_counterpart: bool) -> Vec<Combined> {
    if combs.is_empty() {
        return combs;
    }
    let used: HashSet<(&str, i64)> = combs.iter().map(|c| (c.subs2.text.as_str(), c.subs2.start)).collect();
    let skipped: Vec<&Line> = subs2.iter().filter(|l| !used.contains(&(l.text.as_str(), l.start))).collect();

    let mut groups: HashMap<usize, Vec<Line>> = HashMap::new();
    for skip in skipped {
        let mut best = 0;
        let mut best_overlap = f64::MIN;
        for (i, c) in combs.iter().enumerate() {
            let o = time::overlap(skip.start, skip.end, c.subs1.start, c.subs1.end);
            if o >= best_overlap {
                best = i;
                best_overlap = o;
            }
        }
        if remove_no_counterpart && best_overlap < 0.0 {
            continue;
        }
        groups.entry(best).or_insert_with(|| vec![combs[best].subs2.clone()]).push(skip.clone());
    }
    for (i, mut list) in groups {
        list.sort_by_key(|l| l.start);
        let c = &mut combs[i];
        c.subs2.text = list.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ").trim().to_string();
        c.subs2.start = list[0].start;
        c.subs2.end = list[list.len() - 1].end;
    }
    combs
}

pub fn contains_kanji(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c as u32,
            0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
    })
}

/// Set `active = false` on lines that fail the user's rules.
pub fn apply_filters(episodes: &mut [Episode], p: &Project) {
    let s1 = &p.subs1;
    let s2 = &p.subs2;
    let has_subs2 = p.has_subs2();
    let timing = p.subs(p.timings_from);
    let inc1 = SubsSettings::words(&s1.include_words);
    let exc1 = SubsSettings::words(&s1.exclude_words);
    let inc2 = if has_subs2 { SubsSettings::words(&s2.include_words) } else { vec![] };
    let exc2 = if has_subs2 { SubsSettings::words(&s2.exclude_words) } else { vec![] };
    let actors: Vec<String> = p.actors.iter().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).collect();
    let mut seen1: HashSet<String> = HashSet::new();
    let mut seen2: HashSet<String> = HashSet::new();

    for ep in episodes.iter_mut() {
        for c in ep.lines.iter_mut() {
            let excluded = (|| {
                if let Some(&(a, b)) = p.span.get() {
                    if c.subs1.start < a || c.subs1.start > b {
                        return true;
                    }
                }
                let dur = time::duration(c.subs1.start, c.subs1.end);
                if let Some(&min) = timing.exclude_shorter_ms.get() {
                    if dur < min {
                        return true;
                    }
                }
                if let Some(&max) = timing.exclude_longer_ms.get() {
                    if dur > max {
                        return true;
                    }
                }
                if let Some(&n) = s1.exclude_fewer_chars.get() {
                    if (c.subs1.text.chars().count() as i64) < n {
                        return true;
                    }
                }
                if has_subs2 {
                    if let Some(&n) = s2.exclude_fewer_chars.get() {
                        if (c.subs2.text.chars().count() as i64) < n {
                            return true;
                        }
                    }
                }
                if !actors.is_empty() {
                    let actor = match p.actors_from {
                        Which::Subs1 => &c.subs1.actor,
                        Which::Subs2 => &c.subs2.actor,
                    };
                    if !actors.contains(&actor.trim().to_lowercase()) {
                        return true;
                    }
                }
                let t1 = c.subs1.text.to_lowercase();
                let t2 = c.subs2.text.to_lowercase();
                if exc1.iter().any(|w| t1.contains(w.as_str())) || exc2.iter().any(|w| t2.contains(w.as_str())) {
                    return true;
                }
                if !inc1.is_empty() && !inc1.iter().any(|w| t1.contains(w.as_str())) {
                    return true;
                }
                if !inc2.is_empty() && !inc2.iter().any(|w| t2.contains(w.as_str())) {
                    return true;
                }
                if s1.exclude_duplicates && !seen1.insert(t1.clone()) {
                    return true;
                }
                if has_subs2 && s2.exclude_duplicates && !seen2.insert(t2.clone()) {
                    return true;
                }
                if p.kanji_only && !contains_kanji(&c.subs1.text) && !contains_kanji(&c.subs2.text) {
                    return true;
                }
                false
            })();
            if excluded {
                c.active = false;
            }
        }
    }
}

/// Mark inactive neighbours of active lines as needed for context.
pub fn mark_context(episodes: &mut [Episode], leading: usize, trailing: usize) {
    for ep in episodes.iter_mut() {
        for c in ep.lines.iter_mut() {
            c.context_only = false;
        }
        if leading == 0 && trailing == 0 {
            continue;
        }
        let n = ep.lines.len();
        let active: Vec<bool> = ep.lines.iter().map(|c| c.active).collect();
        for (i, &is_active) in active.iter().enumerate() {
            if !is_active {
                continue;
            }
            let from = i.saturating_sub(leading);
            let to = (i + trailing).min(n.saturating_sub(1));
            for j in (from..i).chain(i + 1..=to) {
                if !active[j] {
                    ep.lines[j].context_only = true;
                }
            }
        }
    }
}

/// Keep lines that are active or (optionally) needed for context.
pub fn retain_lines(episodes: &mut [Episode], keep_context: bool) {
    for ep in episodes.iter_mut() {
        ep.lines.retain(|c| c.active || (keep_context && c.context_only));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: i64, e: i64, t: &str) -> Line {
        Line::new(s, e, t)
    }

    #[test]
    fn join() {
        let v = join_sentences(vec![l(0, 1, "a,"), l(1, 2, "b、"), l(2, 3, "c"), l(4, 5, "d")], ",、");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].text, "a, b、 c");
        assert_eq!(v[0].end, 3);
    }

    #[test]
    fn combine_basic() {
        let p = Project::default();
        let s1 = vec![l(1000, 2000, "こんにちは"), l(3000, 4000, "さようなら"), l(9000, 9500, "alone")];
        let s2 = vec![l(1100, 2100, "Hello"), l(2900, 3500, "Good"), l(3500, 4100, "bye")];
        let c = combine(s1, Some(s2), &p);
        // "alone" has no counterpart → removed (remove_no_counterpart default true)
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].subs2.text, "Hello");
        assert_eq!(c[1].subs2.text, "Good bye", "skipped subs2 line merged in");
        assert_eq!(c[1].subs2.end, 4100);
    }

    #[test]
    fn repeats_merge_subs1() {
        let p = Project::default();
        let s1 = vec![l(0, 1000, "A"), l(1000, 2000, "B")];
        let s2 = vec![l(0, 2000, "AB")];
        let c = combine(s1, Some(s2), &p);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].subs1.text, "A B");
        assert_eq!(c[0].subs1.end, 2000);
    }

    #[test]
    fn timings_from_subs2() {
        let p = Project { timings_from: Which::Subs2, ..Default::default() };
        let c = combine(vec![l(1000, 2000, "x")], Some(vec![l(900, 2100, "y")]), &p);
        assert_eq!((c[0].start(), c[0].end()), (900, 2100));
    }

    #[test]
    fn filters_and_context() {
        let mut p = Project::default();
        p.subs1.exclude_words = "skip".into();
        p.subs1.exclude_duplicates = true;
        let lines = vec![
            Combined::new(l(0, 1000, "keep 1"), Line::default()),
            Combined::new(l(1000, 2000, "please skip"), Line::default()),
            Combined::new(l(2000, 3000, "keep 1"), Line::default()),
            Combined::new(l(3000, 4000, "漢字"), Line::default()),
        ];
        let mut eps = vec![Episode {
            number: 1,
            subs1_file: String::new(),
            subs2_file: None,
            video_file: None,
            audio_file: None,
            lines,
        }];
        apply_filters(&mut eps, &p);
        let act: Vec<bool> = eps[0].lines.iter().map(|c| c.active).collect();
        assert_eq!(act, [true, false, false, true]);
        mark_context(&mut eps, 1, 1);
        let ctx: Vec<bool> = eps[0].lines.iter().map(|c| c.context_only).collect();
        assert_eq!(ctx, [false, true, true, false]);
        retain_lines(&mut eps, false);
        assert_eq!(eps[0].lines.len(), 2);
    }

    #[test]
    fn kanji() {
        assert!(contains_kanji("日本"));
        assert!(!contains_kanji("ひらがな"));
    }
}
