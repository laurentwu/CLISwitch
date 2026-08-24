use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("revision conflict: {0}")]
    Conflict(String),
    #[error("operation is blocked: {0}")]
    Blocked(String),
    #[error("unsupported configuration: {0}")]
    Unsupported(String),
    #[error("I/O operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("network operation failed: {0}")]
    Network(String),
    #[error("operation cancelled")]
    Cancelled,
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<url::ParseError> for AppError {
    fn from(value: url::ParseError) -> Self {
        Self::Validation(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation",
            Self::NotFound(_) => "not-found",
            Self::Conflict(_) => "conflict",
            Self::Blocked(_) => "blocked",
            Self::Unsupported(_) => "unsupported",
            Self::Io(_) => "io",
            Self::Database(_) => "database",
            Self::Migration(_) => "migration",
            Self::Serialization(_) => "serialization",
            Self::Network(_) => "network",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorDto {
            code: self.code(),
            message: crate::services::redaction::sanitize_registered(self.to_string()),
        }
        .serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::redaction::Redactor;

    #[test]
    fn serialized_ipc_errors_apply_the_global_redaction_boundary() {
        let redactor = Redactor::default();
        redactor.register("registered-ipc-secret");
        let serialized = serde_json::to_string(&AppError::Validation(
            "request contained registered-ipc-secret".into(),
        ))
        .unwrap();
        assert!(!serialized.contains("registered-ipc-secret"));
        assert!(serialized.contains("[REDACTED]"));
    }
}
