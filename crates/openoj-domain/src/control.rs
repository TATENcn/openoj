use crate::DomainError;

pub const MAX_UNIX_MILLIS: u64 = 253_402_300_799_999;
pub const MAX_LEASE_DURATION_MS: u64 = 3_600_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnixMillis(u64);

impl UnixMillis {
    /// Creates a bounded Unix timestamp in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the timestamp is later than year 9999.
    pub fn new(value: u64) -> Result<Self, DomainError> {
        if value > MAX_UNIX_MILLIS {
            return Err(DomainError::OutOfRange {
                field: "unix_millis",
                minimum: 0,
                maximum: MAX_UNIX_MILLIS,
                actual: value,
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Adds a bounded lease duration without escaping the supported timestamp range.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the resulting expiry exceeds the timestamp limit.
    pub fn checked_add(self, duration: LeaseDuration) -> Result<Self, DomainError> {
        let actual = self.0 + duration.0;
        if actual > MAX_UNIX_MILLIS {
            return Err(DomainError::OutOfRange {
                field: "lease_expires_at_ms",
                minimum: 0,
                maximum: MAX_UNIX_MILLIS,
                actual,
            });
        }
        Ok(Self(actual))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseDuration(u64);

impl LeaseDuration {
    /// Creates a non-zero bounded lease duration in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] unless the value is within the P0 lease bound.
    pub fn new(value: u64) -> Result<Self, DomainError> {
        if !(1..=MAX_LEASE_DURATION_MS).contains(&value) {
            return Err(DomainError::OutOfRange {
                field: "lease_duration_ms",
                minimum: 1,
                maximum: MAX_LEASE_DURATION_MS,
                actual: value,
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationState {
    Queued,
    Leased,
    Completed,
    Failed,
    Cancelled,
}

impl EvaluationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Validates an Evaluation lifecycle transition.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] for self-transitions, terminal rollback, and skipped states.
    pub fn transition_to(self, next: Self) -> Result<Self, DomainError> {
        let valid = matches!(
            (self, next),
            (Self::Queued, Self::Leased | Self::Cancelled)
                | (
                    Self::Leased,
                    Self::Queued | Self::Completed | Self::Failed | Self::Cancelled
                )
        );
        if !valid {
            return Err(DomainError::InvalidTransition {
                entity: "evaluation",
                from: self.as_str(),
                to: next.as_str(),
            });
        }
        Ok(next)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptState {
    Queued,
    Leased,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

impl AttemptState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    /// Validates an Attempt lifecycle transition.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] for self-transitions, terminal rollback, and skipped states.
    pub fn transition_to(self, next: Self) -> Result<Self, DomainError> {
        let valid = matches!(
            (self, next),
            (Self::Queued, Self::Leased | Self::Cancelled)
                | (
                    Self::Leased,
                    Self::Completed | Self::Failed | Self::Cancelled | Self::Expired
                )
        );
        if !valid {
            return Err(DomainError::InvalidTransition {
                entity: "attempt",
                from: self.as_str(),
                to: next.as_str(),
            });
        }
        Ok(next)
    }
}
