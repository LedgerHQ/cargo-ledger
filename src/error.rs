use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum LedgerError {
    Io(std::io::Error),
    Metadata(cargo_metadata::Error),
    Goblin(goblin::error::Error),
    SerdeJson(serde_json::Error),
    Utf8(std::str::Utf8Error),
    CommandFailure {
        cmd: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    MissingPackage,
    MissingMetadataSection(String),
    MissingField(&'static str),
    Other(String),
}

impl Display for LedgerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::Io(e) => write!(f, "I/O error: {e}"),
            LedgerError::Metadata(e) => write!(f, "cargo metadata error: {e}"),
            LedgerError::Goblin(e) => write!(f, "ELF parse error: {e}"),
            LedgerError::SerdeJson(e) => write!(f, "JSON error: {e}"),
            LedgerError::Utf8(e) => write!(f, "UTF-8 error: {e}"),
            LedgerError::CommandFailure {
                cmd,
                status,
                stderr,
            } => {
                write!(
                    f,
                    "Command '{cmd}' failed (status: {:?}): {}",
                    status,
                    stderr.trim()
                )
            }
            LedgerError::MissingPackage => {
                write!(f, "No package found in metadata result")
            }
            LedgerError::MissingMetadataSection(s) => {
                write!(f, "Missing metadata section: {s}")
            }
            LedgerError::MissingField(fld) => write!(f, "Missing field: {fld}"),
            LedgerError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl Error for LedgerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            LedgerError::Io(e) => Some(e),
            LedgerError::Metadata(e) => Some(e),
            LedgerError::Goblin(e) => Some(e),
            LedgerError::SerdeJson(e) => Some(e),
            LedgerError::Utf8(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for LedgerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<cargo_metadata::Error> for LedgerError {
    fn from(value: cargo_metadata::Error) -> Self {
        Self::Metadata(value)
    }
}
impl From<goblin::error::Error> for LedgerError {
    fn from(value: goblin::error::Error) -> Self {
        Self::Goblin(value)
    }
}
impl From<serde_json::Error> for LedgerError {
    fn from(value: serde_json::Error) -> Self {
        Self::SerdeJson(value)
    }
}
impl From<std::str::Utf8Error> for LedgerError {
    fn from(value: std::str::Utf8Error) -> Self {
        Self::Utf8(value)
    }
}
