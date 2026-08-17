use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use openoj_domain::{
    AttemptId, AttemptState, EvaluationId, EvaluationRequest, EvaluationResult, EvaluationState,
    IdempotencyKey, LeaseDuration, LeaseToken, NodeId, UnixMillis,
};

pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateEvaluation {
    pub request: EvaluationRequest,
    pub created_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimTask {
    pub node_id: NodeId,
    pub lease_token: LeaseToken,
    pub now: UnixMillis,
    pub lease_duration: LeaseDuration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryExpired {
    pub request: EvaluationRequest,
    pub now: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmitResult {
    pub idempotency_key: IdempotencyKey,
    pub lease_token: LeaseToken,
    pub result: EvaluationResult,
    pub now: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelEvaluation {
    pub idempotency_key: IdempotencyKey,
    pub evaluation_id: EvaluationId,
    pub result: EvaluationResult,
    pub now: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationSnapshot {
    pub evaluation_id: EvaluationId,
    pub state: EvaluationState,
    pub current_attempt_id: AttemptId,
    pub attempt_number: u32,
    pub attempt_state: AttemptState,
    pub terminal_result: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskLease {
    pub request: EvaluationRequest,
    pub node_id: NodeId,
    pub lease_token: LeaseToken,
    pub expires_at: UnixMillis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StoreError {
    InvalidTime,
    IdempotencyConflict,
    IdentityConflict,
    ImmutableReferenceConflict,
    NotFound,
    NoTaskAvailable,
    LeaseConflict,
    StaleLease,
    InvalidTransition,
    TerminalConflict,
    IncompatibleSchema,
    Unavailable,
    CorruptData,
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTime => "time value is invalid",
            Self::IdempotencyConflict => "idempotency key conflicts with existing input",
            Self::IdentityConflict => "domain identity conflicts with an existing object",
            Self::ImmutableReferenceConflict => {
                "immutable reference conflicts with existing metadata"
            }
            Self::NotFound => "evaluation was not found",
            Self::NoTaskAvailable => "no evaluation task is available",
            Self::LeaseConflict => "evaluation task lease conflicts with current ownership",
            Self::StaleLease => "evaluation task lease is stale",
            Self::InvalidTransition => "stored state transition is invalid",
            Self::TerminalConflict => "evaluation already has a different terminal result",
            Self::IncompatibleSchema => "database schema is incompatible",
            Self::Unavailable => "storage is unavailable",
            Self::CorruptData => "stored evaluation data is invalid",
        })
    }
}

impl Error for StoreError {}

pub trait EvaluationStore: Send + Sync {
    fn create_evaluation(&self, command: CreateEvaluation) -> StoreFuture<'_, EvaluationSnapshot>;

    fn evaluation_status(&self, evaluation_id: EvaluationId)
    -> StoreFuture<'_, EvaluationSnapshot>;

    fn claim_task(&self, command: ClaimTask) -> StoreFuture<'_, TaskLease>;

    fn retry_expired(&self, command: RetryExpired) -> StoreFuture<'_, EvaluationSnapshot>;

    fn submit_result(&self, command: SubmitResult) -> StoreFuture<'_, EvaluationSnapshot>;

    fn cancel_evaluation(&self, command: CancelEvaluation) -> StoreFuture<'_, EvaluationSnapshot>;
}

pub struct ControlPlane<S> {
    store: S,
}

impl<S> ControlPlane<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S: EvaluationStore> ControlPlane<S> {
    /// Creates or idempotently replays an Evaluation and its first Attempt.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] when persistence rejects or cannot complete the write.
    pub async fn create_evaluation(
        &self,
        command: CreateEvaluation,
    ) -> Result<EvaluationSnapshot, StoreError> {
        self.store.create_evaluation(command).await
    }

    /// Reads the current durable Evaluation and Attempt state.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] when the Evaluation is absent or storage is unavailable.
    pub async fn evaluation_status(
        &self,
        evaluation_id: EvaluationId,
    ) -> Result<EvaluationSnapshot, StoreError> {
        self.store.evaluation_status(evaluation_id).await
    }

    /// Claims at most one ready task under a bounded lease.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] when no task is ready or persistence cannot lease it.
    pub async fn claim_task(&self, command: ClaimTask) -> Result<TaskLease, StoreError> {
        self.store.claim_task(command).await
    }

    /// Replaces an expired Attempt with the explicitly supplied next Attempt.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] for live leases, identity drift, or persistence failure.
    pub async fn retry_expired(
        &self,
        command: RetryExpired,
    ) -> Result<EvaluationSnapshot, StoreError> {
        self.store.retry_expired(command).await
    }

    /// Commits a lease-fenced terminal result.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] for stale leases, terminal conflicts, or storage failure.
    pub async fn submit_result(
        &self,
        command: SubmitResult,
    ) -> Result<EvaluationSnapshot, StoreError> {
        self.store.submit_result(command).await
    }

    /// Commits a canonical cancellation using first-terminal-wins semantics.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] for identity, terminal, or persistence conflicts.
    pub async fn cancel_evaluation(
        &self,
        command: CancelEvaluation,
    ) -> Result<EvaluationSnapshot, StoreError> {
        self.store.cancel_evaluation(command).await
    }
}
