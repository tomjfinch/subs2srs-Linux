use crate::time::Ms;
use serde::{Deserialize, Serialize};

/// One subtitle line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub start: Ms,
    pub end: Ms,
    pub text: String,
    #[serde(default)]
    pub actor: String,
}

impl Line {
    pub fn new(start: Ms, end: Ms, text: impl Into<String>) -> Self {
        Line { start, end, text: text.into(), actor: String::new() }
    }
}

/// A Subs1 line paired with its best matching Subs2 line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combined {
    pub subs1: Line,
    pub subs2: Line,
    pub active: bool,
    /// Inactive, but kept because an active neighbour uses it as context.
    pub context_only: bool,
}

impl Combined {
    pub fn new(subs1: Line, subs2: Line) -> Self {
        Combined { subs1, subs2, active: true, context_only: false }
    }
    /// Timing used for every generated file (always taken from `subs1`
    /// after combining; when "use timings from Subs2" is set, combining copies
    /// the Subs2 times into `subs1`).
    pub fn start(&self) -> Ms {
        self.subs1.start
    }
    pub fn end(&self) -> Ms {
        self.subs1.end
    }
}

/// All matched lines for one episode plus the files they came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// Episode number as shown to the user (`first episode` + index).
    pub number: i64,
    pub subs1_file: String,
    pub subs2_file: Option<String>,
    pub video_file: Option<String>,
    pub audio_file: Option<String>,
    pub lines: Vec<Combined>,
}
