//! Processing core for subs2srs: subtitle parsing, line matching/filtering,
//! file naming, ffmpeg media generation and Anki import file creation.
//!
//! Nothing in this crate depends on GTK, so it can be driven from the GUI,
//! from the bundled `subs2srs-cli` binary or from tests.

pub mod combine;
pub mod encoding;
pub mod error;
pub mod ffmpeg;
pub mod files;
pub mod model;
pub mod naming;
pub mod pipeline;
pub mod progress;
pub mod settings;
pub mod subs;
pub mod time;
pub mod tools;

pub use error::{Error, Result};
