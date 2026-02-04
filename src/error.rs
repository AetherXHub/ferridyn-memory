//! Error types for the ferridyn-memory crate.

use std::fmt;

/// Errors returned by memory backend operations.
#[derive(Debug)]
pub enum MemoryError {
    /// Error from the FerridynDB server client.
    Server(String),
    /// Server socket not found or connection refused.
    ServerUnavailable(String),
    /// Error from a partition schema operation.
    Schema(String),
    /// Error from a secondary index operation.
    Index(String),
    /// Invalid parameters provided by the caller.
    InvalidParams(String),
    /// Internal error during operation.
    Internal(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server(msg) => write!(f, "Server error: {msg}"),
            Self::ServerUnavailable(msg) => write!(f, "Server unavailable: {msg}"),
            Self::Schema(msg) => write!(f, "Schema error: {msg}"),
            Self::Index(msg) => write!(f, "Index error: {msg}"),
            Self::InvalidParams(msg) => write!(f, "Invalid parameters: {msg}"),
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl std::error::Error for MemoryError {}
