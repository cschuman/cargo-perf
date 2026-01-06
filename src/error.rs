//! Custom error types for cargo-perf.
//!
//! Provides structured error handling with clear error categories.

use std::path::PathBuf;
use thiserror::Error;

/// A type alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during cargo-perf operation.
#[derive(Debug, Error)]
pub enum Error {
    /// Failed to parse a Rust source file.
    #[error("Failed to parse {path}: {message}")]
    Parse {
        /// Path to the file that failed to parse.
        path: PathBuf,
        /// Description of the parse error.
        message: String,
    },

    /// Failed to read or access a file.
    #[error("IO error for {path}: {source}")]
    Io {
        /// Path to the file that caused the error.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to load or parse configuration.
    #[error("Configuration error: {message}")]
    Config {
        /// Description of the configuration error.
        message: String,
    },

    /// Generic IO error without path context.
    #[error("IO error: {0}")]
    IoGeneric(#[from] std::io::Error),
}

impl Error {
    /// Create a parse error for a specific file.
    pub fn parse(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Parse {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create an IO error for a specific file.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Create a configuration error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_display() {
        let err = Error::parse("/path/to/file.rs", "unexpected token");
        let msg = err.to_string();
        assert!(msg.contains("/path/to/file.rs"));
        assert!(msg.contains("unexpected token"));
    }

    #[test]
    fn test_io_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::io("/path/to/missing.rs", io_err);
        let msg = err.to_string();
        assert!(msg.contains("/path/to/missing.rs"));
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn test_config_error_display() {
        let err = Error::config("invalid severity level");
        let msg = err.to_string();
        assert!(msg.contains("invalid severity level"));
    }

    #[test]
    fn test_io_generic_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::IoGeneric(_)));
    }
}
