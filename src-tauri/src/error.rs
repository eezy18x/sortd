//! Application-wide error type.
//!
//! Errors cross the IPC boundary as `{ kind, message }`. Messages are kept
//! human-readable and never include file contents.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Busy(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Database(_) => "database",
            AppError::Io(_) => "io",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::NotFound(_) => "not_found",
            AppError::PermissionDenied(_) => "permission_denied",
            AppError::Conflict(_) => "conflict",
            AppError::Busy(_) => "busy",
            AppError::Cancelled => "cancelled",
            AppError::Internal(_) => "internal",
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        AppError::InvalidInput(msg.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
