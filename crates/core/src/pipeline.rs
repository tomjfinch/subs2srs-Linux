//! The main job: subtitles → matched lines → Anki TSV + audio/snapshots/video.

use crate::combine;
use crate::ffmpeg;
use crate::files::{self, AUDIO_EXTS, VIDEO_EXTS};
use crate::model::{Combined, Episode};
use crate::naming::{sanitize_filename, Namer};
use crate::progress::Progress;
use crate::settings::{AudioSource, ContextSide, Prefs, Project, SubsSettings, VideoCodec};
use crate::subs::{self, ParseOptions, CONTAINER_EXTS, SUBTITLE_EXTS};
use crate::time::{self, Ms};
use crate::{bail, Error, Result};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub subs1: Vec<PathBuf>,
    pub subs2: Vec<PathBuf>,
    pub video: Vec<PathBuf>,
    pub audio: Vec<PathBuf>,
}

impl Inputs {
    pub fn episodes(&self) -> usize {
        self.subs1.len()
    }
}

pub fn subtitle_exts() -> Vec<&'static str> {
    SUBTITLE_EXTS.iter().chain(CONTAINER_EXTS.iter()).copied().collect()
}

/// Expand all file patterns and check that the counts line up.
pub fn resolve_inputs(p: &Project) -> Result<Inputs> {
    let sub_exts = subtitle_exts();
    let mut inp = Inputs {
        subs1: files::expand(&p.subs1.pattern, &sub_exts),
        subs2: files::expand(&p.subs2.pattern, &sub_exts),
        video: files::expand(&p.video_pattern, VIDEO_EXTS),
        audio: files::expand(&p.audio.pattern, AUDIO_EXTS),
    };
    if p.subs1.pattern.trim().is_empty() {
        bail!("Choose the Subs1 (target language) subtitles first.");
    }
    if inp.subs1.is_empty() {
        bail!("No subtitle files match Subs1: {}", p.subs1.pattern);
    }
    let n = inp.subs1.len();
    if p.has_subs2() && inp.subs2.len() != n {
        bail!("Subs1 has {n} file(s) but Subs2 has {}. Each episode needs one of each.", inp.subs2.len());
    }
    if p.needs_video() {
        if p.video_pattern.trim().is_empty() {
            bail!("Snapshots, video clips and audio-from-video need the video files. Choose them or turn those off.");
        }
        if inp.video.len() != n {
            bail!("Subs1 has {n} file(s) but {} video file(s) were found.", inp.video.len());
        }
    }
    if p.audio.enabled && p.audio.source == AudioSource::Files && inp.audio.len() != n {
        bail!("Subs1 has {n} file(s) but {} audio file(s) were found.", inp.audio.len());
    }
    if p.episode_limit > 0 && p.episode_limit < n {
        let k = p.episode_limit;
        inp.subs1.truncate(k);
        inp.subs2.truncate(k);
        inp.video.truncate(k);
        inp.audio.truncate(k);
    }
    Ok(inp)
}

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("subs2srs-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Parse one subtitle input (a subtitle file or a container with an embedded track).
pub fn read_track(path: &Path, s: &SubsSettings, opts: ParseOptions, progress: Option<&Progress>) -> Result<Vec<crate::model::Line>> {
    if subs::is_container(path) {
        let out = temp_dir().join(format!(
            "track-{}-{}.ass",
            s.embedded_track,
            path.file_name().map(|n| n.to_string_lossy().replace(' ', "_")).unwrap_or_default()
        ));
        ffmpeg::extract_subtitle(path, s.embedded_track, &out, progress)?;
        let r = subs::parse_file(&out, "utf-8", opts);
        let _ = std::fs::remove_file(&out);
        r
    } else {
        subs::parse_file(path, &s.encoding, opts)
    }
}

/// Parse + combine every episode. No filtering yet (all lines active).
pub fn load_episodes(p: &Project, inp: &Inputs, progress: &Progress, keep_line_breaks: bool) -> Result<Vec<Episode>> {
    let n = inp.episodes();
    let mut eps = Vec::with_capacity(n);
    for i in 0..n {
        progress.check()?;
        progress.update(Some(i as f64 / n as f64), format!("Reading subtitles for episode {} of {n}", i + 1));
        let number = p.first_episode + i as i64;
        let o1 = ParseOptions { remove_styled: p.subs1.remove_styled, keep_line_breaks };
        let s1 = read_track(&inp.subs1[i], &p.subs1, o1, Some(progress))?;
        let s1 = combine::prepare_track(s1, &p.subs1, p.time_shift_enabled, number);
        let s2 = if p.has_subs2() {
            let o2 = ParseOptions { remove_styled: p.subs2.remove_styled, keep_line_breaks };
            let s2 = read_track(&inp.subs2[i], &p.subs2, o2, Some(progress))?;
            Some(combine::prepare_track(s2, &p.subs2, p.time_shift_enabled, number))
        } else {
            None
        };
        let lines = combine::combine(s1, s2, p);
        eps.push(Episode {
            number,
            subs1_file: inp.subs1[i].display().to_string(),
            subs2_file: inp.subs2.get(i).map(|f| f.display().to_string()),
            video_file: inp.video.get(i).map(|f| f.display().to_string()),
            audio_file: inp.audio.get(i).map(|f| f.display().to_string()),
            lines,
        });
    }
    let total: usize = eps.iter().map(|e| e.lines.len()).sum();
    if total == 0 {
        bail!("No dialogue lines could be read from the subtitle files. Check the files and the encoding.");
    }
    Ok(eps)
}

/// Load and apply the filters (used by Preview and Generate).
pub fn prepare(p: &Project, progress: &Progress) -> Result<(Inputs, Vec<Episode>)> {
    let inp = resolve_inputs(p)?;
    let mut eps = load_episodes(p, &inp, progress, false)?;
    combine::apply_filters(&mut eps, p);
    Ok((inp, eps))
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub tsv_path: PathBuf,
    pub media_dir: PathBuf,
    pub cards: usize,
    pub audio_clips: usize,
    pub snapshots: usize,
    pub video_clips: usize,
    pub skipped_existing: usize,
    pub elapsed_secs: f64,
    pub columns: Vec<String>,
}

impl Summary {
    pub fn message(&self) -> String {
        let mut s = format!("{} cards written to {}\n", self.cards, self.tsv_path.display());
        if self.audio_clips + self.snapshots + self.video_clips > 0 {
            let _ = writeln!(
                s,
                "Media in {}: {} audio, {} snapshots, {} video clips",
                self.media_dir.display(),
                self.audio_clips,
                self.snapshots,
                self.video_clips
            );
        }
        if self.skipped_existing > 0 {
            let _ = writeln!(s, "{} media files already existed and were kept.", self.skipped_existing);
        }
        let _ = writeln!(s, "\nColumns: {}", self.columns.join(" · "));
        let _ = write!(s, "\nFinished in {:.1} s. Copy the files from the .media folder into Anki's collection.media folder, then import the .tsv.", self.elapsed_secs);
        s
    }
}

/// A line that will be written out, with its sequence number and episode.
#[derive(Debug, Clone)]
struct Item {
    ep: usize,
    idx: usize,
}

struct Ctx<'a> {
    p: &'a Project,
    prefs: &'a Prefs,
    eps: &'a [Episode],
    seqs: Vec<Vec<i64>>,
    namer: Namer,
    media_dir: PathBuf,
}

impl<'a> Ctx<'a> {
    fn line(&self, ep: usize, idx: usize) -> &Combined {
        &self.eps[ep].lines[idx]
    }
    fn name(&self, fmt: &str, ep: usize, idx: usize, start: Ms, end: Ms) -> String {
        let c = self.line(ep, idx);
        let s2 = if self.p.has_subs2() { c.subs2.text.trim() } else { "" };
        self.namer.create(fmt, self.eps[ep].number, self.seqs[ep][idx], start, end, &c.subs1.text, s2)
    }
    fn audio_times(&self, c: &Combined) -> (Ms, Ms) {
        self.p.audio.pad.apply(c.start(), c.end())
    }
    fn video_times(&self, c: &Combined) -> (Ms, Ms) {
        self.p.video_clips.pad.apply(c.start(), c.end())
    }
    fn audio_file(&self, ep: usize, idx: usize) -> String {
        let (s, e) = self.audio_times(self.line(ep, idx));
        let base = self.name(&self.prefs.audio_filename_format, ep, idx, s, e);
        sanitize_filename(&format!("{base}.{}", self.p.audio.format.ext()))
    }
    fn snapshot_file(&self, ep: usize, idx: usize) -> String {
        let c = self.line(ep, idx);
        sanitize_filename(&self.name(&self.prefs.snapshot_filename_format, ep, idx, c.start(), c.end()))
    }
    fn video_file(&self, ep: usize, idx: usize) -> String {
        let (s, e) = self.video_times(self.line(ep, idx));
        let base = self.name(&self.prefs.video_filename_format, ep, idx, s, e);
        sanitize_filename(&format!("{base}.{}", self.p.video_clips.codec.ext()))
    }
    fn wrap(&self, prefix: &str, file: &str, suffix: &str, ep: usize, idx: usize) -> String {
        let c = self.line(ep, idx);
        let pre = self.name(prefix, ep, idx, c.start(), c.end());
        let suf = self.name(suffix, ep, idx, c.start(), c.end());
        format!("{pre}{file}{suf}")
    }

    /// Fields for one line. `side` = None for the card's own line.
    fn fields(&self, ep: usize, idx: usize, side: Option<&ContextSide>) -> Vec<String> {
        let p = self.p;
        let pr = self.prefs;
        let c = self.line(ep, idx);
        let mut f = Vec::new();
        if side.is_none() {
            if !pr.srs_tag_format.is_empty() {
                f.push(self.name(&pr.srs_tag_format, ep, idx, c.start(), c.end()));
            }
            if !pr.srs_sequence_marker_format.is_empty() {
                f.push(self.name(&pr.srs_sequence_marker_format, ep, idx, c.start(), c.end()));
            }
        }
        let want = |inc: fn(&ContextSide) -> bool| side.map(inc).unwrap_or(true);
        if p.audio.enabled && want(|s| s.include_audio) {
            f.push(self.wrap(&pr.audio_prefix, &self.audio_file(ep, idx), &pr.audio_suffix, ep, idx));
        }
        if p.snapshots.enabled && want(|s| s.include_snapshots) {
            f.push(self.wrap(&pr.snapshot_prefix, &self.snapshot_file(ep, idx), &pr.snapshot_suffix, ep, idx));
        }
        if p.video_clips.enabled && want(|s| s.include_video) {
            f.push(self.wrap(&pr.video_prefix, &self.video_file(ep, idx), &pr.video_suffix, ep, idx));
        }
        f.push(clean_field(&self.name(&pr.srs_subs1_format, ep, idx, c.start(), c.end()), &pr.srs_delimiter));
        if p.has_subs2() {
            f.push(clean_field(&self.name(&pr.srs_subs2_format, ep, idx, c.start(), c.end()), &pr.srs_delimiter));
        }
        f
    }

    fn placeholder(&self, side: &ContextSide) -> Vec<String> {
        let p = self.p;
        let mut n = 1;
        if p.audio.enabled && side.include_audio {
            n += 1;
        }
        if p.snapshots.enabled && side.include_snapshots {
            n += 1;
        }
        if p.video_clips.enabled && side.include_video {
            n += 1;
        }
        if p.has_subs2() {
            n += 1;
        }
        vec![String::new(); n]
    }
}

fn clean_field(s: &str, delim: &str) -> String {
    let s = s.replace(['\r', '\n'], " ");
    if delim.is_empty() {
        s
    } else {
        s.replace(delim, " ")
    }
}

/// Is the context line `ctx` close enough to `base` (no gap > range)?
fn in_range(lines: &[Combined], base: usize, ctx: usize, range_secs: i64) -> bool {
    if range_secs <= 0 {
        return true;
    }
    let max = range_secs * 1000;
    let (a, b) = if ctx < base { (ctx, base) } else { (base, ctx) };
    (a..b).all(|i| time::duration(lines[i].end(), lines[i + 1].start()) <= max)
}

pub fn columns(p: &Project, prefs: &Prefs) -> Vec<String> {
    let mut c = Vec::new();
    if !prefs.srs_tag_format.is_empty() {
        c.push("Tag".to_string());
    }
    if !prefs.srs_sequence_marker_format.is_empty() {
        c.push("Sequence marker".into());
    }
    if p.audio.enabled {
        c.push("Audio".into());
    }
    if p.snapshots.enabled {
        c.push("Snapshot".into());
    }
    if p.video_clips.enabled {
        c.push("Video".into());
    }
    c.push("Subs1".into());
    if p.has_subs2() {
        c.push("Subs2".into());
    }
    let l = &p.context_leading;
    if l.count > 0 {
        c.push(format!("+{} leading line{}", l.count, if l.count == 1 { "" } else { "s" }));
    }
    let t = &p.context_trailing;
    if t.count > 0 {
        c.push(format!("+{} trailing line{}", t.count, if t.count == 1 { "" } else { "s" }));
    }
    c
}

fn anki_separator(d: &str) -> Option<&'static str> {
    Some(match d {
        "\t" => "tab",
        "," => "comma",
        ";" => "semicolon",
        "|" => "pipe",
        ":" => "colon",
        " " => "space",
        _ => return None,
    })
}

/// Run everything. `episodes` may come from the preview window (with the
/// user's edits); otherwise they're loaded and filtered here.
pub fn generate(p: &Project, prefs: &Prefs, episodes: Option<Vec<Episode>>, progress: &Progress) -> Result<Summary> {
    let started = Instant::now();
    if p.deck_name.trim().is_empty() {
        bail!("Enter a deck name.");
    }
    if p.output_dir.trim().is_empty() {
        bail!("Choose an output folder.");
    }
    let media_wanted = p.audio.enabled || p.snapshots.enabled || p.video_clips.enabled;
    let any_container = files::expand(&p.subs1.pattern, &subtitle_exts()).iter().any(|f| subs::is_container(f));
    if media_wanted || any_container {
        ffmpeg::available()?;
    }

    let mut steps = 2;
    steps += p.audio.enabled as usize + p.snapshots.enabled as usize + p.video_clips.enabled as usize;
    progress.set_steps(steps);
    progress.next_step("Reading subtitles");

    let mut eps = match episodes {
        Some(e) => {
            // Preview may be stale regarding inputs; re-validate counts.
            resolve_inputs(p)?;
            e
        }
        None => prepare(p, progress)?.1,
    };

    let lead = &p.context_leading;
    let trail = &p.context_trailing;
    combine::mark_context(&mut eps, lead.count, trail.count);
    combine::retain_lines(&mut eps, true);
    let cards: usize = eps.iter().map(|e| e.lines.iter().filter(|c| c.active).count()).sum();
    if cards == 0 {
        bail!("No lines are left after filtering. Check the filters (span, include/exclude words, actors…).");
    }

    // Global sequence numbers over everything that's written.
    let mut seq = 0;
    let seqs: Vec<Vec<i64>> = eps
        .iter()
        .map(|e| {
            e.lines
                .iter()
                .map(|_| {
                    seq += 1;
                    seq
                })
                .collect()
        })
        .collect();
    let total_lines = seq as usize;
    let last_time = eps.iter().flat_map(|e| e.lines.iter().map(|c| c.end())).max().unwrap_or(0);
    let namer = Namer::new(&p.clean_deck_name(), eps.len(), total_lines, last_time)
        .with_size(p.video_clips.width as i64, p.video_clips.height as i64);

    let out_dir = PathBuf::from(files::expand_tilde(p.output_dir.trim()));
    let mut proj = p.clone();
    proj.output_dir = out_dir.display().to_string();
    let media_dir = proj.media_dir();
    std::fs::create_dir_all(&out_dir).map_err(|e| Error::msg(format!("Cannot create {}: {e}", out_dir.display())))?;
    if media_wanted {
        std::fs::create_dir_all(&media_dir)
            .map_err(|e| Error::msg(format!("Cannot create {}: {e}", media_dir.display())))?;
    }
    let ctx = Ctx { p: &proj, prefs, eps: &eps, seqs, namer, media_dir: media_dir.clone() };

    // ── TSV ────────────────────────────────────────────────────────────
    progress.next_step("Writing Anki import file");
    let tsv_name = sanitize_filename(&ctx.namer.create(&prefs.srs_filename_format, 0, 0, 0, 0, "", ""));
    let tsv_path = out_dir.join(tsv_name);
    let delim = if prefs.srs_delimiter.is_empty() { "\t" } else { prefs.srs_delimiter.as_str() };
    let mut tsv = String::new();
    if prefs.anki_headers {
        if let Some(sep) = anki_separator(delim) {
            let _ = writeln!(tsv, "#separator:{sep}");
            let _ = writeln!(tsv, "#html:true");
            if !prefs.srs_tag_format.is_empty() {
                let _ = writeln!(tsv, "#tags column:1");
            }
        }
    }
    for (ei, ep) in eps.iter().enumerate() {
        for (li, c) in ep.lines.iter().enumerate() {
            if !c.active {
                continue;
            }
            let mut f = ctx.fields(ei, li, None);
            for k in (1..=lead.count).rev() {
                match li.checked_sub(k) {
                    Some(j) if in_range(&ep.lines, li, j, lead.range_secs) => f.extend(ctx.fields(ei, j, Some(lead))),
                    _ => f.extend(ctx.placeholder(lead)),
                }
            }
            for k in 1..=trail.count {
                let j = li + k;
                if j < ep.lines.len() && in_range(&ep.lines, li, j, trail.range_secs) {
                    f.extend(ctx.fields(ei, j, Some(trail)));
                } else {
                    f.extend(ctx.placeholder(trail));
                }
            }
            tsv.push_str(&f.join(delim));
            tsv.push('\n');
        }
    }
    std::fs::write(&tsv_path, tsv).map_err(|e| Error::msg(format!("Cannot write {}: {e}", tsv_path.display())))?;
    progress.update(Some(1.0), "");

    let mut summary = Summary {
        tsv_path,
        media_dir: media_dir.clone(),
        cards,
        columns: columns(p, prefs),
        ..Default::default()
    };

    // Which lines need media of a given kind.
    let needs = |inc: fn(&ContextSide) -> bool| -> Vec<Vec<Item>> {
        eps.iter()
            .enumerate()
            .map(|(ei, ep)| {
                ep.lines
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.active || (c.context_only && (inc(lead) && lead.count > 0 || inc(trail) && trail.count > 0)))
                    .map(|(li, _)| Item { ep: ei, idx: li })
                    .collect()
            })
            .collect()
    };

    let tmp_root = out_dir.join(format!(".subs2srs-tmp-{}", std::process::id()));
    let result = (|| -> Result<()> {
        if p.audio.enabled {
            progress.next_step("Creating audio clips");
            let (made, skipped) = gen_audio(&ctx, &needs(|s| s.include_audio), &tmp_root, progress)?;
            summary.audio_clips = made;
            summary.skipped_existing += skipped;
        }
        if p.snapshots.enabled {
            progress.next_step("Creating snapshots");
            let (made, skipped) = gen_snapshots(&ctx, &needs(|s| s.include_snapshots), progress)?;
            summary.snapshots = made;
            summary.skipped_existing += skipped;
        }
        if p.video_clips.enabled {
            progress.next_step("Creating video clips");
            let (made, skipped) = gen_video(&ctx, &needs(|s| s.include_video), progress)?;
            summary.video_clips = made;
            summary.skipped_existing += skipped;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp_root);
    let _ = std::fs::remove_dir_all(temp_dir());
    result?;
    summary.elapsed_secs = started.elapsed().as_secs_f64();
    Ok(summary)
}

/// Run `f` over `items` on `threads` workers. Stops at the first error.
fn parallel<T: Sync>(
    items: &[T],
    threads: usize,
    progress: &Progress,
    f: impl Fn(&T) -> Result<()> + Sync,
    report: impl Fn(usize, usize) + Sync,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let err: Mutex<Option<Error>> = Mutex::new(None);
    let n = items.len();
    std::thread::scope(|s| {
        for _ in 0..threads.clamp(1, n) {
            s.spawn(|| loop {
                if stop.load(Ordering::SeqCst) || progress.is_cancelled() {
                    break;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= n {
                    break;
                }
                if let Err(e) = f(&items[i]) {
                    stop.store(true, Ordering::SeqCst);
                    let mut g = err.lock().unwrap();
                    if g.is_none() || matches!(*g, Some(Error::Cancelled)) {
                        *g = Some(e);
                    }
                    break;
                }
                let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                report(d, n);
            });
        }
    });
    progress.check()?;
    match err.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn finish_tmp(tmp: &Path, out: &Path) -> Result<()> {
    std::fs::rename(tmp, out).map_err(|e| Error::msg(format!("Cannot write {}: {e}", out.display())))
}

fn tmp_name(out: &Path) -> PathBuf {
    let ext = out.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    out.with_extension(format!("tmp.{ext}"))
}

fn gen_audio(ctx: &Ctx, per_ep: &[Vec<Item>], tmp_root: &Path, progress: &Progress) -> Result<(usize, usize)> {
    let p = ctx.p;
    let total: usize = per_ep.iter().map(Vec::len).sum();
    let mut made = 0;
    let mut skipped = 0;
    let mut done_before = 0;
    std::fs::create_dir_all(tmp_root)?;

    for items in per_ep.iter().filter(|v| !v.is_empty()) {
        progress.check()?;
        let ep = &ctx.eps[items[0].ep];
        let todo: Vec<(&Item, PathBuf)> = items
            .iter()
            .map(|it| (it, ctx.media_dir.join(ctx.audio_file(it.ep, it.idx))))
            .filter(|(_, path)| !path.exists())
            .collect();
        skipped += items.len() - todo.len();
        if todo.is_empty() {
            done_before += items.len();
            continue;
        }
        let (src, map) = match p.audio.source {
            AudioSource::Video => (ep.video_file.clone().unwrap_or_default(), format!("0:a:{}", p.audio_stream)),
            AudioSource::Files => (ep.audio_file.clone().unwrap_or_default(), "0:a:0".to_string()),
        };
        // 1. Decode the span covering every clip to a WAV once.
        let span_start = todo.iter().map(|(it, _)| ctx.audio_times(ctx.line(it.ep, it.idx)).0).min().unwrap();
        let span_end = todo.iter().map(|(it, _)| ctx.audio_times(ctx.line(it.ep, it.idx)).1).max().unwrap();
        let wav = tmp_root.join(format!("ep{}.wav", ep.number));
        let mut args: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            ffmpeg::ss(span_start),
            "-i".into(),
            src.clone(),
            "-t".into(),
            ffmpeg::ss(span_end - span_start),
            "-map".into(),
            map,
            "-vn".into(),
            "-sn".into(),
            "-dn".into(),
        ];
        if p.audio.normalize {
            args.extend(["-af".into(), "loudnorm=I=-16:TP=-1.5:LRA=11".into()]);
        }
        args.extend(["-ac".into(), "2".into(), "-ar".into(), "48000".into(), "-c:a".into(), "pcm_s16le".into(), wav.display().to_string()]);
        let span = (span_end - span_start).max(1);
        let base = done_before;
        let label = format!("Decoding audio of episode {}", ep.number);
        let mut on_time = |t: Ms| {
            let pct = (t as f64 / span as f64 * 100.0).clamp(0.0, 100.0);
            progress.update(Some(base as f64 / total as f64), format!("{label} ({pct:.0}%)"));
        };
        ffmpeg::run(&args, Some(progress), Some(&mut on_time)).map_err(|e| match e {
            Error::Msg(m) => Error::msg(format!(
                "Could not extract audio from {src}. Check the audio stream selection, and that the file isn't DRM-protected.\n\n{m}"
            )),
            e => e,
        })?;

        // 2. Cut + encode clips in parallel.
        let nclips = todo.len();
        let already = base + (items.len() - nclips);
        let fmt = p.audio.format;
        let res = parallel(
            &todo,
            ctx.prefs.threads(),
            progress,
            |(it, out)| {
                let c = ctx.line(it.ep, it.idx);
                let (s, e) = ctx.audio_times(c);
                let tmp = tmp_name(out);
                let tag = |f: &str| ctx.name(f, it.ep, it.idx, s, e);
                let mut a: Vec<String> = vec![
                    "-y".into(),
                    "-ss".into(),
                    ffmpeg::ss(s - span_start),
                    "-t".into(),
                    ffmpeg::ss(e - s),
                    "-i".into(),
                    wav.display().to_string(),
                    "-map".into(),
                    "0:a".into(),
                    "-c:a".into(),
                    fmt.codec().into(),
                    "-b:a".into(),
                    format!("{}k", p.audio.bitrate),
                    "-map_metadata".into(),
                    "-1".into(),
                ];
                let meta = [
                    ("artist", tag(&ctx.prefs.audio_tag_artist)),
                    ("album", tag(&ctx.prefs.audio_tag_album)),
                    ("title", tag(&ctx.prefs.audio_tag_title)),
                    ("genre", tag(&ctx.prefs.audio_tag_genre)),
                    ("lyrics", tag(&ctx.prefs.audio_tag_lyrics).trim().to_string()),
                    ("track", format!("{}/{}", it.idx + 1, ep.lines.len())),
                ];
                for (k, v) in meta {
                    if !v.is_empty() {
                        a.push("-metadata".into());
                        a.push(format!("{k}={v}"));
                    }
                }
                a.push(tmp.display().to_string());
                ffmpeg::run(&a, Some(progress), None)?;
                finish_tmp(&tmp, out)
            },
            |d, _| {
                let k = already + d;
                progress.update(Some(k as f64 / total as f64), format!("Audio clip {k} of {total}"));
            },
        );
        let _ = std::fs::remove_file(&wav);
        res?;
        made += nclips;
        done_before += items.len();
    }
    Ok((made, skipped))
}

fn gen_snapshots(ctx: &Ctx, per_ep: &[Vec<Item>], progress: &Progress) -> Result<(usize, usize)> {
    let s = &ctx.p.snapshots;
    let all: Vec<(&Item, PathBuf)> = per_ep
        .iter()
        .flatten()
        .map(|it| (it, ctx.media_dir.join(ctx.snapshot_file(it.ep, it.idx))))
        .collect();
    let todo: Vec<(&Item, PathBuf)> = all.iter().filter(|(_, p)| !p.exists()).cloned().collect();
    let skipped = all.len() - todo.len();
    let filter = ffmpeg::video_filter(s.width, s.height, &s.crop, false);
    let n = todo.len();
    let report = |d: usize, _| progress.update(Some(d as f64 / n as f64), format!("Snapshot {d} of {n}"));
    parallel(&todo, ctx.prefs.threads(), progress, |(it, out)| {
        let c = ctx.line(it.ep, it.idx);
        let video = ctx.eps[it.ep].video_file.clone().unwrap_or_default();
        let tmp = tmp_name(out);
        let ext = out.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let mut a: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            ffmpeg::ss(time::midpoint(c.start(), c.end())),
            "-i".into(),
            video,
            "-map".into(),
            "0:v:0".into(),
            "-an".into(),
            "-sn".into(),
            "-dn".into(),
            "-frames:v".into(),
            "1".into(),
            "-vf".into(),
            filter.clone(),
            "-threads".into(),
            "1".into(),
        ];
        match ext.as_str() {
            "jpg" | "jpeg" => a.extend(["-q:v".into(), s.quality.clamp(2, 31).to_string()]),
            "webp" => a.extend(["-quality".into(), (100 - (s.quality.clamp(2, 31) - 2) * 3).to_string()]),
            _ => {}
        }
        a.extend(["-update".into(), "1".into(), tmp.display().to_string()]);
        ffmpeg::run(&a, Some(progress), None)?;
        finish_tmp(&tmp, out)
    }, report)?;
    Ok((todo.len(), skipped))
}

fn gen_video(ctx: &Ctx, per_ep: &[Vec<Item>], progress: &Progress) -> Result<(usize, usize)> {
    let v = &ctx.p.video_clips;
    let all: Vec<(&Item, PathBuf)> = per_ep
        .iter()
        .flatten()
        .map(|it| (it, ctx.media_dir.join(ctx.video_file(it.ep, it.idx))))
        .collect();
    let todo: Vec<(&Item, PathBuf)> = all.iter().filter(|(_, p)| !p.exists()).cloned().collect();
    let skipped = all.len() - todo.len();
    let filter = ffmpeg::video_filter(v.width, v.height, &v.crop, true);
    let audio_map = match ctx.p.audio.source {
        AudioSource::Video => format!("0:a:{}?", ctx.p.audio_stream),
        AudioSource::Files => "0:a:0?".into(),
    };
    let threads = (ctx.prefs.threads() / 2).max(1);
    let n = todo.len();
    let report = |d: usize, _| progress.update(Some(d as f64 / n as f64), format!("Video clip {d} of {n}"));
    parallel(&todo, threads, progress, |(it, out)| {
        let c = ctx.line(it.ep, it.idx);
        let (s, e) = ctx.video_times(c);
        let video = ctx.eps[it.ep].video_file.clone().unwrap_or_default();
        let tmp = tmp_name(out);
        let mut a: Vec<String> = vec![
            "-y".into(),
            "-ss".into(),
            ffmpeg::ss(s),
            "-i".into(),
            video,
            "-t".into(),
            ffmpeg::ss(e - s),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            audio_map.clone(),
            "-sn".into(),
            "-dn".into(),
            "-map_metadata".into(),
            "-1".into(),
            "-vf".into(),
            filter.clone(),
            "-b:v".into(),
            format!("{}k", v.bitrate_video),
            "-b:a".into(),
            format!("{}k", v.bitrate_audio),
            "-ac".into(),
            "2".into(),
            "-threads".into(),
            "2".into(),
        ];
        match v.codec {
            VideoCodec::Mp4 => a.extend(
                ["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+faststart"]
                    .map(String::from),
            ),
            VideoCodec::Webm => a.extend(
                ["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-row-mt", "1", "-c:a", "libopus"]
                    .map(String::from),
            ),
            VideoCodec::Avi => a.extend(["-c:v", "mpeg4", "-c:a", "libmp3lame"].map(String::from)),
        }
        a.push(tmp.display().to_string());
        ffmpeg::run(&a, Some(progress), None)?;
        finish_tmp(&tmp, out)
    }, report)?;
    Ok((todo.len(), skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Line;

    #[test]
    fn range() {
        let mk = |s, e| Combined::new(Line::new(s, e, "x"), Line::default());
        let v = vec![mk(0, 1000), mk(2000, 3000), mk(30_000, 31_000)];
        assert!(in_range(&v, 1, 0, 15));
        assert!(!in_range(&v, 1, 2, 15));
        assert!(in_range(&v, 1, 2, 0));
    }
}
