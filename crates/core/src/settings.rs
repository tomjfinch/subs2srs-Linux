//! Project settings (saved as `.s2s.json`) and global preferences
//! (`~/.config/subs2srs/preferences.json`).

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Which {
    #[default]
    Subs1,
    Subs2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, bound(deserialize = "T: Deserialize<'de> + Default"))]
pub struct Toggle<T> {
    pub enabled: bool,
    pub value: T,
}

impl<T> Toggle<T> {
    pub fn new(enabled: bool, value: T) -> Self {
        Toggle { enabled, value }
    }
    pub fn get(&self) -> Option<&T> {
        self.enabled.then_some(&self.value)
    }
}

impl<T: Default> Default for Toggle<T> {
    fn default() -> Self {
        Toggle { enabled: false, value: T::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShiftRule {
    /// Applies to this episode number and every later one (until the next rule).
    pub from_episode: i64,
    pub shift_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SubsSettings {
    /// File, directory or glob (`/anime/show/*.ass`). May point at video
    /// containers, in which case `embedded_track` is extracted with ffmpeg.
    pub pattern: String,
    pub encoding: String,
    /// Index among the container's subtitle streams (0 = first).
    pub embedded_track: u32,
    pub time_shift_ms: i64,
    pub shift_rules: Vec<ShiftRule>,
    /// `;`-separated words. Lines must contain one of them.
    pub include_words: String,
    /// `;`-separated words. Lines containing any of them are dropped.
    pub exclude_words: String,
    pub remove_no_counterpart: bool,
    pub remove_styled: bool,
    pub exclude_duplicates: bool,
    pub exclude_fewer_chars: Toggle<i64>,
    pub exclude_shorter_ms: Toggle<i64>,
    pub exclude_longer_ms: Toggle<i64>,
    pub join_sentences: Toggle<String>,
}

impl Default for SubsSettings {
    fn default() -> Self {
        SubsSettings {
            pattern: String::new(),
            encoding: "auto".into(),
            embedded_track: 0,
            time_shift_ms: 0,
            shift_rules: Vec::new(),
            include_words: String::new(),
            exclude_words: String::new(),
            remove_no_counterpart: true,
            remove_styled: true,
            exclude_duplicates: false,
            exclude_fewer_chars: Toggle::new(false, 8),
            exclude_shorter_ms: Toggle::new(false, 800),
            exclude_longer_ms: Toggle::new(false, 5000),
            join_sentences: Toggle::new(true, ",、→".into()),
        }
    }
}

impl SubsSettings {
    /// Shift for an episode: the global shift, overridden by the last rule
    /// whose `from_episode` is <= `episode` (rules are cascading).
    pub fn effective_shift(&self, episode: i64) -> i64 {
        let mut rules: Vec<&ShiftRule> = self.shift_rules.iter().collect();
        rules.sort_by_key(|r| r.from_episode);
        let mut shift = self.time_shift_ms;
        for r in rules {
            if r.from_episode <= episode {
                shift = r.shift_ms;
            } else {
                break;
            }
        }
        shift
    }

    pub fn words(list: &str) -> Vec<String> {
        list.split(';').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Crop {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl Crop {
    pub fn is_empty(&self) -> bool {
        self.top == 0 && self.bottom == 0 && self.left == 0 && self.right == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pad {
    pub enabled: bool,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Default for Pad {
    fn default() -> Self {
        Pad { enabled: false, start_ms: 250, end_ms: 250 }
    }
}

impl Pad {
    pub fn apply(&self, start: i64, end: i64) -> (i64, i64) {
        if self.enabled {
            (crate::time::pad(start, -self.start_ms), crate::time::pad(end, self.end_ms))
        } else {
            (start, end)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    #[default]
    Opus,
    Mp3,
    Aac,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 3] = [AudioFormat::Opus, AudioFormat::Mp3, AudioFormat::Aac];
    pub fn ext(self) -> &'static str {
        match self {
            AudioFormat::Opus => "opus",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Aac => "m4a",
        }
    }
    pub fn codec(self) -> &'static str {
        match self {
            AudioFormat::Opus => "libopus",
            AudioFormat::Mp3 => "libmp3lame",
            AudioFormat::Aac => "aac",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::Opus => "Opus",
            AudioFormat::Mp3 => "MP3",
            AudioFormat::Aac => "AAC (.m4a)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioSource {
    #[default]
    Video,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub enabled: bool,
    pub source: AudioSource,
    /// External audio files (when `source == Files`).
    pub pattern: String,
    pub format: AudioFormat,
    pub bitrate: u32,
    pub pad: Pad,
    /// EBU R128 loudness normalisation (ffmpeg `loudnorm`).
    pub normalize: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings {
            enabled: true,
            source: AudioSource::Video,
            pattern: String::new(),
            format: AudioFormat::Opus,
            bitrate: 128,
            pad: Pad::default(),
            normalize: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SnapshotSettings {
    pub enabled: bool,
    pub width: u32,
    /// 0 = keep aspect ratio.
    pub height: u32,
    pub crop: Crop,
    /// JPEG quality, 2 (best) .. 31 (worst).
    pub quality: u32,
}

impl Default for SnapshotSettings {
    fn default() -> Self {
        SnapshotSettings { enabled: true, width: 480, height: 0, crop: Crop::default(), quality: 3 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    /// H.264 + AAC in MP4. Plays everywhere, including AnkiMobile.
    #[default]
    Mp4,
    /// VP9 + Opus in WebM.
    Webm,
    /// MPEG-4 Part 2 + MP3 in AVI (what the original produced by default).
    Avi,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 3] = [VideoCodec::Mp4, VideoCodec::Webm, VideoCodec::Avi];
    pub fn ext(self) -> &'static str {
        match self {
            VideoCodec::Mp4 => "mp4",
            VideoCodec::Webm => "webm",
            VideoCodec::Avi => "avi",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::Mp4 => "MP4 (H.264 + AAC)",
            VideoCodec::Webm => "WebM (VP9 + Opus)",
            VideoCodec::Avi => "AVI (MPEG-4 + MP3)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoClipSettings {
    pub enabled: bool,
    pub codec: VideoCodec,
    pub width: u32,
    /// 0 = keep aspect ratio.
    pub height: u32,
    pub crop: Crop,
    pub bitrate_video: u32,
    pub bitrate_audio: u32,
    pub pad: Pad,
}

impl Default for VideoClipSettings {
    fn default() -> Self {
        VideoClipSettings {
            enabled: false,
            codec: VideoCodec::Mp4,
            width: 480,
            height: 0,
            crop: Crop::default(),
            bitrate_video: 800,
            bitrate_audio: 128,
            pad: Pad::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ContextSide {
    pub count: usize,
    /// Neighbours further away than this many seconds aren't used (0 = no limit).
    pub range_secs: i64,
    pub include_audio: bool,
    pub include_snapshots: bool,
    pub include_video: bool,
}

impl Default for ContextSide {
    fn default() -> Self {
        ContextSide { count: 0, range_secs: 15, include_audio: false, include_snapshots: false, include_video: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Project {
    pub deck_name: String,
    pub output_dir: String,
    pub first_episode: i64,
    /// Only process this many episodes (0 = all).
    pub episode_limit: usize,
    pub subs1: SubsSettings,
    pub subs2: SubsSettings,
    /// Video files (one per episode).
    pub video_pattern: String,
    /// Index among the video's audio streams.
    pub audio_stream: u32,
    pub timings_from: Which,
    pub time_shift_enabled: bool,
    pub span: Toggle<(i64, i64)>,
    pub audio: AudioSettings,
    pub snapshots: SnapshotSettings,
    pub video_clips: VideoClipSettings,
    pub context_leading: ContextSide,
    pub context_trailing: ContextSide,
    /// Only keep lines spoken by these actors (empty = everyone).
    pub actors: Vec<String>,
    pub actors_from: Which,
    /// Japanese: keep only lines containing at least one kanji.
    pub kanji_only: bool,
}

impl Default for Project {
    fn default() -> Self {
        let mut subs2 = SubsSettings::default();
        subs2.join_sentences.enabled = true;
        Project {
            deck_name: String::new(),
            output_dir: String::new(),
            first_episode: 1,
            episode_limit: 0,
            subs1: SubsSettings::default(),
            subs2,
            video_pattern: String::new(),
            audio_stream: 0,
            timings_from: Which::Subs1,
            time_shift_enabled: false,
            span: Toggle::new(false, (90_000, 1_350_000)),
            audio: AudioSettings::default(),
            snapshots: SnapshotSettings::default(),
            video_clips: VideoClipSettings::default(),
            context_leading: ContextSide::default(),
            context_trailing: ContextSide::default(),
            actors: Vec::new(),
            actors_from: Which::Subs1,
            kanji_only: false,
        }
    }
}

impl Project {
    pub fn subs(&self, which: Which) -> &SubsSettings {
        match which {
            Which::Subs1 => &self.subs1,
            Which::Subs2 => &self.subs2,
        }
    }
    pub fn has_subs2(&self) -> bool {
        !self.subs2.pattern.trim().is_empty()
    }
    /// Deck names become file names and Anki tags, so no spaces.
    pub fn clean_deck_name(&self) -> String {
        self.deck_name.trim().replace([' ', '/', '\\'], "_")
    }
    pub fn media_dir(&self) -> PathBuf {
        Path::new(&self.output_dir).join(format!("{}.media", self.clean_deck_name()))
    }
    pub fn needs_video(&self) -> bool {
        self.snapshots.enabled
            || self.video_clips.enabled
            || (self.audio.enabled && self.audio.source == AudioSource::Video)
    }

    pub fn load(path: &Path) -> Result<Project> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_string_pretty(self)?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the desktop (freedesktop colour-scheme portal / GTK settings).
    #[default]
    System,
    Light,
    Dark,
}

/// Global preferences: naming formats and app behaviour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Prefs {
    pub theme: Theme,
    pub window_width: i32,
    pub window_height: i32,
    /// 0 = number of CPU threads.
    pub max_parallel: usize,
    /// Command used to preview audio; empty = auto (mpv, ffplay, pw-play, paplay).
    pub audio_player: String,
    pub last_dir: String,

    pub srs_filename_format: String,
    pub srs_delimiter: String,
    pub srs_tag_format: String,
    pub srs_sequence_marker_format: String,
    pub srs_subs1_format: String,
    pub srs_subs2_format: String,
    /// Write `#separator`, `#html` and `#tags column` headers that Anki
    /// (2.1.55+) understands, so the import dialog is pre-configured.
    pub anki_headers: bool,

    pub audio_prefix: String,
    pub audio_suffix: String,
    /// Without extension; it comes from the audio format.
    pub audio_filename_format: String,
    pub audio_tag_artist: String,
    pub audio_tag_album: String,
    pub audio_tag_title: String,
    pub audio_tag_genre: String,
    pub audio_tag_lyrics: String,

    pub snapshot_prefix: String,
    pub snapshot_suffix: String,
    /// With extension (`.jpg`, `.webp`, `.png`, `.avif`).
    pub snapshot_filename_format: String,

    pub video_prefix: String,
    pub video_suffix: String,
    /// Without extension; it comes from the video codec.
    pub video_filename_format: String,

    pub extract_audio_filename_format: String,
    pub extract_lyrics_subs1_format: String,
    pub extract_lyrics_subs2_format: String,

    pub dueling_filename_format: String,
    pub dueling_quickref_filename_format: String,
    pub dueling_quickref_subs1_format: String,
    pub dueling_quickref_subs2_format: String,
}

const TS_SE: &str = "${0:s_total_hour}.${0:s_min}.${0:s_sec}.${0:s_msec}-${0:e_total_hour}.${0:e_min}.${0:e_sec}.${0:e_msec}";

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: Theme::System,
            window_width: 980,
            window_height: 720,
            max_parallel: 0,
            audio_player: String::new(),
            last_dir: String::new(),
            srs_filename_format: "${deck_name}.tsv".into(),
            srs_delimiter: "\t".into(),
            srs_tag_format: "${deck_name}_${0:episode_num}".into(),
            srs_sequence_marker_format: "${0:episode_num}_${0:sequence_num}_${0:s_total_hour}.${0:s_min}.${0:s_sec}.${0:s_msec}".into(),
            srs_subs1_format: "${subs1_line}".into(),
            srs_subs2_format: "${subs2_line}".into(),
            anki_headers: true,
            audio_prefix: "[sound:".into(),
            audio_suffix: "]".into(),
            audio_filename_format: format!("${{deck_name}}_${{0:episode_num}}_{TS_SE}"),
            audio_tag_artist: "${deck_name}".into(),
            audio_tag_album: "${deck_name}_${0:episode_num}".into(),
            audio_tag_title: format!("${{deck_name}}_${{0:episode_num}}_{TS_SE}"),
            audio_tag_genre: "Speech".into(),
            audio_tag_lyrics: "${subs1_line} ${subs2_line}".into(),
            snapshot_prefix: "<img src=\"".into(),
            snapshot_suffix: "\">".into(),
            snapshot_filename_format: "${deck_name}_${0:episode_num}_${0:m_total_hour}.${0:m_min}.${0:m_sec}.${0:m_msec}.jpg".into(),
            video_prefix: "[sound:".into(),
            video_suffix: "]".into(),
            video_filename_format: format!("${{deck_name}}_${{0:episode_num}}_{TS_SE}"),
            extract_audio_filename_format: format!("${{deck_name}}_${{0:episode_num}}_{TS_SE}"),
            extract_lyrics_subs1_format: "[${2:s_total_min}:${2:s_sec}.${2:s_hsec}] ${subs1_line}".into(),
            extract_lyrics_subs2_format: "[${2:s_total_min}:${2:s_sec}.${2:s_hsec}] ${subs2_line}".into(),
            dueling_filename_format: "${deck_name}_${0:episode_num}.ass".into(),
            dueling_quickref_filename_format: "${deck_name}_${0:episode_num}.txt".into(),
            dueling_quickref_subs1_format: "[${0:s_total_hour}:${0:s_min}:${0:s_sec}.${0:s_hsec}]  ${subs1_line}".into(),
            dueling_quickref_subs2_format: "[${0:s_total_hour}:${0:s_min}:${0:s_sec}.${0:s_hsec}]  ${subs2_line}\n".into(),
        }
    }
}

impl Prefs {
    pub fn threads(&self) -> usize {
        if self.max_parallel > 0 {
            self.max_parallel
        } else {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        }
    }
}

/// `$XDG_CONFIG_HOME/subs2srs` (or `~/.config/subs2srs`).
pub fn config_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        return PathBuf::from(x).join("subs2srs");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("subs2srs")
}

pub fn prefs_path() -> PathBuf {
    config_dir().join("preferences.json")
}

/// Project defaults saved with "Save as default".
pub fn default_project_path() -> PathBuf {
    config_dir().join("default-project.s2s.json")
}

impl Prefs {
    pub fn load() -> Prefs {
        std::fs::read_to_string(prefs_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<()> {
        write_atomic(&prefs_path(), &serde_json::to_string_pretty(self)?)
    }
}

pub fn load_default_project() -> Project {
    Project::load(&default_project_path()).unwrap_or_default()
}

pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_rules_cascade() {
        let mut s = SubsSettings { time_shift_ms: 100, ..Default::default() };
        s.shift_rules = vec![
            ShiftRule { from_episode: 5, shift_ms: 500 },
            ShiftRule { from_episode: 3, shift_ms: 300 },
        ];
        assert_eq!(s.effective_shift(1), 100);
        assert_eq!(s.effective_shift(3), 300);
        assert_eq!(s.effective_shift(4), 300);
        assert_eq!(s.effective_shift(9), 500);
    }

    #[test]
    fn roundtrip() {
        let p = Project { deck_name: "Show".into(), ..Default::default() };
        let j = serde_json::to_string(&p).unwrap();
        let q: Project = serde_json::from_str(&j).unwrap();
        assert_eq!(p, q);
        // Missing fields fall back to defaults.
        let r: Project = serde_json::from_str("{\"deck_name\":\"X\"}").unwrap();
        assert_eq!(r.first_episode, 1);
        assert!(r.audio.enabled);
    }
}
