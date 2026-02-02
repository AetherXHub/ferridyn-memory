//! Error types for the ferridyn-memory crate.

use std::fmt;

/// Errors returned by memory backend operations.
#[derive(Debug)]
pub enum MemoryError {
    /// Error from the FerridynDB core database engine.
    Database(String),
    /// Error from the FerridynDB server client.
    Server(String),
    /// Invalid parameters provided by the caller.
    InvalidParams(String),
    /// Internal error during operation.
    Internal(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(msg) => write!(f, "Database error: {msg}"),
            Self::Server(msg) => write!(f, "Server error: {msg}"),
            Self::InvalidParams(msg) => write!(f, "Invalid parameters: {msg}"),
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl std::error::Error for MemoryError {}
