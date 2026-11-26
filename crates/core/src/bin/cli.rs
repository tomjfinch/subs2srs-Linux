//! Headless front-end: `subs2srs-cli generate project.s2s.json`.

use std::path::PathBuf;
use std::sync::Arc;
use subs2srs_core::pipeline;
use subs2srs_core::progress::Progress;
use subs2srs_core::settings::{Prefs, Project};
use subs2srs_core::{ffmpeg, time, tools, Error};

const USAGE: &str = "subs2srs-cli — create Anki cards from subtitles and video

USAGE:
    subs2srs-cli generate <project.s2s.json> [--prefs <preferences.json>]
    subs2srs-cli preview  <project.s2s.json>      list matched lines, * = active
    subs2srs-cli dueling  <project.s2s.json>      write dual-language .ass files
    subs2srs-cli extract-audio <media pattern> <output dir> <name> [chunk seconds]
    subs2srs-cli probe    <media file>            list audio/subtitle streams
    subs2srs-cli defaults                         print a default project file

Projects are the .s2s.json files saved by the subs2srs GUI.
Preferences default to ~/.config/subs2srs/preferences.json.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(Error::Cancelled) => 130,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> subs2srs_core::Result<()> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let mut prefs = Prefs::load();
    if let Some(i) = args.iter().position(|a| a == "--prefs") {
        let path = args.get(i + 1).ok_or_else(|| Error::msg("--prefs needs a file"))?;
        prefs = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    }
    let project = || -> subs2srs_core::Result<Project> {
        let path = args.get(1).ok_or_else(|| Error::msg(format!("missing project file\n\n{USAGE}")))?;
        Project::load(&PathBuf::from(path))
    };
    match cmd {
        "generate" => {
            let p = project()?;
            let progress = Arc::new(Progress::new());
            let summary = with_progress(&progress, || pipeline::generate(&p, &prefs, None, &progress))?;
            println!("{}", summary.message());
        }
        "preview" => {
            let p = project()?;
            let progress = Progress::new();
            let (_, eps) = pipeline::prepare(&p, &progress)?;
            for ep in eps {
                println!("── Episode {} ({})", ep.number, ep.subs1_file);
                for c in ep.lines {
                    println!(
                        "{} {} → {}  {}  |  {}",
                        if c.active { '*' } else { ' ' },
                        time::to_hms_ms(c.start()),
                        time::to_hms_ms(c.end()),
                        c.subs1.text,
                        c.subs2.text
                    );
                }
            }
        }
        "dueling" => {
            let p = project()?;
            let progress = Progress::new();
            for f in tools::dueling(&p, &prefs, &tools::DuelingSettings::default(), &progress)? {
                println!("{}", f.display());
            }
        }
        "extract-audio" => {
            let get = |i: usize| args.get(i).cloned().ok_or_else(|| Error::msg(format!("missing argument\n\n{USAGE}")));
            let x = tools::ExtractAudioSettings {
                media_pattern: get(1)?,
                output_dir: get(2)?,
                name: get(3)?,
                clip_length_ms: args.get(4).and_then(|s| s.parse::<i64>().ok()).map(|s| s * 1000).unwrap_or(0),
                ..Default::default()
            };
            let progress = Arc::new(Progress::new());
            for f in with_progress(&progress, || tools::extract_audio(&x, &prefs, &progress))? {
                println!("{}", f.display());
            }
        }
        "probe" => {
            let f = args.get(1).ok_or_else(|| Error::msg("missing file"))?;
            let info = ffmpeg::probe(&PathBuf::from(f))?;
            println!("Duration: {}", time::to_hms_ms(info.duration));
            for v in &info.video {
                println!("Video    {} {}x{}", v.describe(), v.width, v.height);
            }
            for a in &info.audio {
                println!("Audio    {}", a.describe());
            }
            for s in &info.subtitles {
                println!("Subtitle {}", s.describe());
            }
        }
        "defaults" => println!("{}", serde_json::to_string_pretty(&Project::default())?),
        _ => {
            println!("{USAGE}");
        }
    }
    Ok(())
}

fn with_progress<T>(progress: &Arc<Progress>, f: impl FnOnce() -> T) -> T {
    let p = progress.clone();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let d = done.clone();
    let printer = std::thread::spawn(move || {
        let mut last = String::new();
        while !d.load(std::sync::atomic::Ordering::SeqCst) {
            let s = p.snapshot();
            let line = format!(
                "[{}/{}] {} {}{}",
                s.step,
                s.steps,
                s.step_name,
                s.detail,
                p.overall().map(|f| format!(" ({:.0}%)", f * 100.0)).unwrap_or_default()
            );
            if line != last {
                eprintln!("{line}");
                last = line;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
    let r = f();
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = printer.join();
    r
}
