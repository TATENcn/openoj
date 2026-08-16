use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DomainError {
    Empty {
        field: &'static str,
    },
    TooLong {
        field: &'static str,
        maximum: usize,
        actual: usize,
    },
    InvalidFormat {
        field: &'static str,
        reason: &'static str,
    },
    OutOfRange {
        field: &'static str,
        minimum: u64,
        maximum: u64,
        actual: u64,
    },
    TooManyItems {
        field: &'static str,
        maximum: usize,
        actual: usize,
    },
    DuplicateItem {
        field: &'static str,
    },
    InvalidPlan,
    InvalidScore,
    InvalidResult {
        reason: &'static str,
    },
    InvalidProvenance {
        reason: &'static str,
    },
}

impl Display for DomainError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(formatter, "{field} must not be empty"),
            Self::TooLong {
                field,
                maximum,
                actual,
            } => write!(
                formatter,
                "{field} exceeds its length limit ({actual} > {maximum})"
            ),
            Self::InvalidFormat { field, reason } => {
                write!(formatter, "{field} has an invalid format: {reason}")
            }
            Self::OutOfRange {
                field,
                minimum,
                maximum,
                actual,
            } => write!(
                formatter,
                "{field} is outside the allowed range ({actual} not in {minimum}..={maximum})"
            ),
            Self::TooManyItems {
                field,
                maximum,
                actual,
            } => write!(
                formatter,
                "{field} contains too many items ({actual} > {maximum})"
            ),
            Self::DuplicateItem { field } => {
                write!(formatter, "{field} contains a duplicate item")
            }
            Self::InvalidPlan => formatter.write_str("evaluation plan is invalid"),
            Self::InvalidScore => formatter.write_str("score is invalid"),
            Self::InvalidResult { reason } => {
                write!(formatter, "evaluation result is invalid: {reason}")
            }
            Self::InvalidProvenance { reason } => {
                write!(formatter, "execution provenance is invalid: {reason}")
            }
        }
    }
}

impl Error for DomainError {}
