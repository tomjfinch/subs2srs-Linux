use std::fmt;

/// Error type for everything in the core. Messages are meant to be shown to
/// the user as-is.
#[derive(Debug, Clone)]
pub enum Error {
    Msg(String),
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Error::Msg(s.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Msg(m) => f.write_str(m),
            Error::Cancelled => f.write_str("Cancelled"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Msg(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Msg(format!("JSON error: {e}"))
    }
}

/// `bail!("...")` shorthand.
#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::Error::Msg(format!($($t)*))) };
}
