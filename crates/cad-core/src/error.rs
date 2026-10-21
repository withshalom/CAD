//! Error type shared by the whole core.

use std::fmt;

/// Broad category of a failure. Callers mostly display the message, but the
/// kind lets a UI decide whether to offer "open anyway" or report a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Reading the file from disk failed.
    Io,
    /// The file is not a DWG file, or the version code is unknown.
    NotDwg,
    /// A DWG version we recognise but do not read yet (for example R2007).
    UnsupportedVersion,
    /// A feature inside a supported file that is not implemented yet
    /// (for example password-encrypted sections).
    Unsupported,
    /// Ran past the end of a buffer or bit stream.
    Truncated,
    /// Data does not match the structure the specification describes.
    Format,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
        }
    }
    pub fn format(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Format, message)
    }
    pub fn truncated(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Truncated, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Unsupported, message)
    }
    /// Prefix the message with context, keeping the kind.
    pub fn context(mut self, ctx: impl fmt::Display) -> Self {
        self.message = format!("{ctx}: {}", self.message);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            ErrorKind::Io => "I/O error",
            ErrorKind::NotDwg => "not a DWG file",
            ErrorKind::UnsupportedVersion => "unsupported DWG version",
            ErrorKind::Unsupported => "unsupported feature",
            ErrorKind::Truncated => "truncated data",
            ErrorKind::Format => "format error",
        };
        write!(f, "{kind}: {}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::new(ErrorKind::Io, e.to_string())
    }
}

/// Attach context to any `Result<_, Error>`.
pub trait ResultExt<T> {
    fn ctx(self, ctx: impl fmt::Display) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn ctx(self, ctx: impl fmt::Display) -> Result<T> {
        self.map_err(|e| e.context(ctx))
    }
}
