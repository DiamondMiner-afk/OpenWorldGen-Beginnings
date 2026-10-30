use crate::contracts::ValidationIssue;
use thiserror::Error;

/// Errors are explicit because a plausible-looking partial result is unsafe.
#[derive(Debug, Error)]
pub enum OwgError {
    #[error("scenario validation failed with {0} issue(s)")]
    Validation(usize, Vec<ValidationIssue>),
    #[error("accounting entry is not balanced: residual {residual}")]
    UnbalancedEntry { residual: i64 },
    #[error("money arithmetic overflow")]
    MoneyOverflow,
    #[error("unknown account: {0}")]
    UnknownAccount(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("checkpoint is incompatible: {0}")]
    IncompatibleCheckpoint(String),
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("CSV parsing failed: {0}")]
    Csv(#[from] csv::Error),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub type OwgResult<T> = Result<T, OwgError>;
