//! The engine's error type. Every failure a user or agent can cause is one of these, never a panic.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// A table, page, command or record that doesn't exist.
    NotFound,
    /// Bad parameters or a value that fails validation (AL `Error`/`TestField`).
    Invalid,
    /// A business rule refused the action (blocked customer, status not Open, unbalanced posting).
    Refused,
    /// I/O, data-file or database problems.
    Io,
    /// Another session changed the record since it was read (optimistic concurrency).
    Conflict,
    /// The user may not run this command or touch this table.
    Permission,
    /// A bug: a panic was caught and the change rolled back.
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
}

impl EngineError {
    pub fn not_found(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::NotFound, message: m.into() }
    }
    pub fn invalid(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Invalid, message: m.into() }
    }
    pub fn refused(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Refused, message: m.into() }
    }
    pub fn io(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Io, message: m.into() }
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Conflict, message: m.into() }
    }
    pub fn permission(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Permission, message: m.into() }
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Internal, message: m.into() }
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            ErrorKind::NotFound => "not_found",
            ErrorKind::Invalid => "invalid",
            ErrorKind::Refused => "refused",
            ErrorKind::Io => "io",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Permission => "permission",
            ErrorKind::Internal => "internal",
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

impl From<String> for EngineError {
    fn from(m: String) -> Self {
        EngineError::invalid(m)
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// `Err(EngineError::refused(..))` with `format!` arguments.
#[macro_export]
macro_rules! refuse {
    ($($arg:tt)*) => {
        return Err($crate::EngineError::refused(format!($($arg)*)))
    };
}
