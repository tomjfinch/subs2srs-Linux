//! Running ffmpeg/ffprobe.

use crate::progress::Progress;
use crate::time::{self, Ms};
use crate::{bail, Error, Result};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// `$SUBS2SRS_FFMPEG` or `ffmpeg` from `$PATH`.
pub fn ffmpeg_bin() -> PathBuf {
    std::env::var_os("SUBS2SRS_FFMPEG").map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into())
}

pub fn ffprobe_bin() -> PathBuf {
    std::env::var_os("SUBS2SRS_FFPROBE").map(PathBuf::from).unwrap_or_else(|| "ffprobe".into())
}

/// Find an executable in `$PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() > 1 {
        return p.is_file().then(|| p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).map(|d| d.join(name)).find(|f| f.is_file())
    })
}

pub fn available() -> Result<()> {
    for (bin, name) in [(ffmpeg_bin(), "ffmpeg"), (ffprobe_bin(), "ffprobe")] {
        if which(&bin.to_string_lossy()).is_none() {
            bail!("{name} was not found. Install it (e.g. `emerge media-video/ffmpeg`) and make sure it is in your PATH.");
        }
    }
    Ok(())
}

/// Encoders compiled into this ffmpeg (`libopus`, `libx264`, …).
pub fn has_encoder(name: &str) -> bool {
    Command::new(ffmpeg_bin())
        .args(["-hide_banner", "-encoders"])
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().any(|w| w == name))
        .unwrap_or(false)
}

/// Run ffmpeg with `args` (without the leading binary). Kills the process if
/// `progress` is cancelled. `on_time` receives the processed output time.
pub fn run(args: &[String], progress: Option<&Progress>, mut on_time: Option<&mut dyn FnMut(Ms)>) -> Result<()> {
    let mut cmd = Command::new(ffmpeg_bin());
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error"]);
    if on_time.is_some() {
        cmd.args(["-progress", "pipe:1", "-nostats"]);
    }
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::msg(format!("Could not start ffmpeg ({}): {e}", ffmpeg_bin().display())))?;

    let mut stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Ms>();
    let out_thread = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if let Some(v) = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms=")) {
                if let Ok(us) = v.trim().parse::<i64>() {
                    let _ = tx.send(us / 1000);
                }
            }
        }
    });

    let status = loop {
        while let Ok(t) = rx.try_recv() {
            if let Some(cb) = on_time.as_mut() {
                cb(t);
            }
        }
        if let Some(p) = progress {
            if p.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err(Error::Cancelled);
            }
        }
        match child.try_wait()? {
            Some(st) => break st,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let _ = out_thread.join();
    let err = err_thread.join().unwrap_or_default();
    if !status.success() {
        let last: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = last[last.len().saturating_sub(6)..].join("\n");
        bail!("ffmpeg failed ({status}):\n{tail}\n\nCommand: ffmpeg {}", shell_join(args));
    }
    Ok(())
}

pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "'\"$\\;&|()<>*?".contains(c)) {
                format!("'{}'", a.replace('\'', "'\\''"))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Default)]
pub struct StreamInfo {
    /// Absolute stream index in the file.
    pub index: u32,
    /// Index among streams of the same type (what `0:a:N` / `0:s:N` uses).
    pub type_index: u32,
    pub codec: String,
    pub language: String,
    pub title: String,
    pub default: bool,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
}

impl StreamInfo {
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("#{}", self.type_index + 1)];
        if !self.language.is_empty() {
            parts.push(language_name(&self.language));
        }
        if !self.title.is_empty() {
            parts.push(format!("“{}”", self.title));
        }
        let mut tech = self.codec.clone();
        if self.channels > 0 {
            tech.push_str(&format!(", {}ch", self.channels));
        }
        parts.push(format!("({tech})"));
        if self.default {
            parts.push("default".into());
        }
        parts.join(" ")
    }

    /// Subtitle codecs we can convert to ASS/SRT (bitmap subs can't).
    pub fn is_text_subtitle(&self) -> bool {
        !matches!(self.codec.as_str(), "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub")
    }
}

#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub duration: Ms,
    pub video: Vec<StreamInfo>,
    pub audio: Vec<StreamInfo>,
    pub subtitles: Vec<StreamInfo>,
}

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}
#[derive(Deserialize)]
struct ProbeFormat {
    #[serde(default)]
    duration: Option<String>,
}
#[derive(Deserialize, Default)]
struct ProbeStream {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    channels: Option<u32>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
    #[serde(default)]
    disposition: std::collections::HashMap<String, i64>,
}

pub fn probe(file: &Path) -> Result<MediaInfo> {
    let out = Command::new(ffprobe_bin())
        .args(["-v", "error", "-print_format", "json", "-show_streams", "-show_format"])
        .arg(file)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::msg(format!("Could not start ffprobe: {e}")))?;
    if !out.status.success() {
        bail!("ffprobe could not read {}:\n{}", file.display(), String::from_utf8_lossy(&out.stderr).trim());
    }
    let p: ProbeOut = serde_json::from_slice(&out.stdout)?;
    let mut info = MediaInfo {
        duration: p
            .format
            .and_then(|f| f.duration)
            .and_then(|d| d.parse::<f64>().ok())
            .map(|d| (d * 1000.0) as Ms)
            .unwrap_or(0),
        ..Default::default()
    };
    for s in p.streams {
        let tag = |k: &str| {
            s.tags.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default()
        };
        let list = match s.codec_type.as_str() {
            "video" => {
                // Cover art is reported as a video stream; skip it.
                if s.disposition.get("attached_pic").copied().unwrap_or(0) == 1 {
                    continue;
                }
                &mut info.video
            }
            "audio" => &mut info.audio,
            "subtitle" => &mut info.subtitles,
            _ => continue,
        };
        let si = StreamInfo {
            index: s.index,
            type_index: list.len() as u32,
            codec: s.codec_name.clone(),
            language: tag("language"),
            title: tag("title"),
            default: s.disposition.get("default").copied().unwrap_or(0) == 1,
            width: s.width.unwrap_or(0),
            height: s.height.unwrap_or(0),
            channels: s.channels.unwrap_or(0),
        };
        list.push(si);
    }
    Ok(info)
}

/// Video filter string: crop (source pixels) then scale. Height 0 keeps aspect.
pub fn video_filter(width: u32, height: u32, crop: &crate::settings::Crop, even: bool) -> String {
    let mut f = Vec::new();
    if !crop.is_empty() {
        f.push(format!(
            "crop=iw-{}:ih-{}:{}:{}",
            crop.left + crop.right,
            crop.top + crop.bottom,
            crop.left,
            crop.top
        ));
    }
    if width > 0 || height > 0 {
        let auto = if even { "-2" } else { "-1" };
        let w = if width > 0 { (if even { width & !1 } else { width }).to_string() } else { auto.to_string() };
        let h = if height > 0 { (if even { height & !1 } else { height }).to_string() } else { auto.to_string() };
        f.push(format!("scale={w}:{h}"));
    }
    if f.is_empty() {
        "null".into()
    } else {
        f.join(",")
    }
}

pub fn ss(t: Ms) -> String {
    time::to_ffmpeg(t)
}

/// Extract an embedded text subtitle track to ASS so it can be parsed.
pub fn extract_subtitle(container: &Path, track: u32, out: &Path, progress: Option<&Progress>) -> Result<()> {
    let args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        container.display().to_string(),
        "-map".into(),
        format!("0:s:{track}"),
        "-c:s".into(),
        "ass".into(),
        out.display().to_string(),
    ];
    run(&args, progress, None).map_err(|e| match e {
        Error::Cancelled => e,
        Error::Msg(m) => Error::msg(format!(
            "Could not extract subtitle track #{} from {}. Bitmap subtitles (PGS/VobSub) are not supported.\n\n{m}",
            track + 1,
            container.display()
        )),
    })
}

/// ISO 639-2/639-1 code → English name for the common languages.
pub fn language_name(code: &str) -> String {
    let c = code.to_ascii_lowercase();
    let name = match c.as_str() {
        "jpn" | "ja" => "Japanese",
        "eng" | "en" => "English",
        "spa" | "es" => "Spanish",
        "fre" | "fra" | "fr" => "French",
        "ger" | "deu" | "de" => "German",
        "ita" | "it" => "Italian",
        "por" | "pt" => "Portuguese",
        "rus" | "ru" => "Russian",
        "chi" | "zho" | "zh" => "Chinese",
        "kor" | "ko" => "Korean",
        "ara" | "ar" => "Arabic",
        "pol" | "pl" => "Polish",
        "dut" | "nld" | "nl" => "Dutch",
        "swe" | "sv" => "Swedish",
        "tur" | "tr" => "Turkish",
        "ukr" | "uk" => "Ukrainian",
        "vie" | "vi" => "Vietnamese",
        "tha" | "th" => "Thai",
        "ind" | "id" => "Indonesian",
        "hin" | "hi" => "Hindi",
        "und" => "Unknown",
        _ => return code.to_string(),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Crop;

    #[test]
    fn filters() {
        assert_eq!(video_filter(480, 0, &Crop::default(), true), "scale=480:-2");
        let c = Crop { top: 0, bottom: 60, left: 0, right: 0 };
        assert_eq!(video_filter(241, 0, &c, true), "crop=iw-0:ih-60:0:0,scale=240:-2");
        assert_eq!(video_filter(0, 0, &Crop::default(), false), "null");
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_join(&["-i".into(), "a b.mkv".into()]), "-i 'a b.mkv'");
    }
}
