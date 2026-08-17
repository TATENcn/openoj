use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::{collections::BTreeMap, collections::BTreeSet};

use openoj_domain::{
    AttemptId, AttemptState, Capability, ClaimOperationId, EvaluationId, EvaluationRequest,
    EvaluationResult, EvaluationState, IdempotencyKey, LeaseDuration, LeaseToken, NodeId,
    UnixMillis,
};

/// P0-C control-plane policy for a lease and its server-scheduled renewal interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeasePolicy {
    lease_duration: LeaseDuration,
    renew_after: LeaseDuration,
}

impl LeasePolicy {
    /// Creates a bounded server-owned lease policy.
    ///
    /// # Errors
    ///
    /// Returns [`NodePolicyError::InvalidRenewSchedule`] when renewals would occur after half of
    /// the lease interval.
    pub fn new(
        lease_duration: LeaseDuration,
        renew_after: LeaseDuration,
    ) -> Result<Self, NodePolicyError> {
        if renew_after.value() > lease_duration.value() / 2 {
            return Err(NodePolicyError::InvalidRenewSchedule);
        }
        Ok(Self {
            lease_duration,
            renew_after,
        })
    }

    #[must_use]
    pub const fn lease_duration(self) -> LeaseDuration {
        self.lease_duration
    }

    #[must_use]
    pub const fn renew_after(self) -> LeaseDuration {
        self.renew_after
    }
}

/// Control-plane allowlist for locally deployed P0-C judge nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodePolicy {
    allowed: BTreeMap<NodeId, BTreeSet<Capability>>,
}

impl NodePolicy {
    /// Creates a default-deny node allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`NodePolicyError::DuplicateNode`] or [`NodePolicyError::InvalidCapabilities`] for
    /// ambiguous deployment configuration.
    pub fn new(
        nodes: impl IntoIterator<Item = (NodeId, Vec<Capability>)>,
    ) -> Result<Self, NodePolicyError> {
        let mut allowed = BTreeMap::new();
        for (node_id, capabilities) in nodes {
            let capability_count = capabilities.len();
            let unique = capabilities.into_iter().collect::<BTreeSet<_>>();
            if unique.is_empty() || unique.len() > 64 || unique.len() != capability_count {
                return Err(NodePolicyError::InvalidCapabilities);
            }
            if allowed.insert(node_id, unique).is_some() {
                return Err(NodePolicyError::DuplicateNode);
            }
        }
        Ok(Self { allowed })
    }

    /// Authorizes a node's declared capabilities against its deployment allowlist.
    ///
    /// # Errors
    ///
    /// Returns a default-deny [`NodePolicyError`] for unknown nodes or capability expansion.
    pub fn authorize(
        &self,
        node_id: &NodeId,
        declared: &[Capability],
    ) -> Result<(), NodePolicyError> {
        let allowed = self
            .allowed
            .get(node_id)
            .ok_or(NodePolicyError::IdentityDenied)?;
        if declared.is_empty() {
            return Err(NodePolicyError::CapabilityDenied);
        }
        let unique_declared = declared.iter().collect::<BTreeSet<_>>();
        if unique_declared.len() != declared.len()
            || unique_declared
                .iter()
                .any(|capability| !allowed.contains(*capability))
        {
            return Err(NodePolicyError::CapabilityDenied);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodePolicyError {
    InvalidRenewSchedule,
    DuplicateNode,
    InvalidCapabilities,
    IdentityDenied,
    CapabilityDenied,
}

impl Display for NodePolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRenewSchedule => "renew schedule exceeds half of the lease duration",
            Self::DuplicateNode => "node policy contains a duplicate node",
            Self::InvalidCapabilities => "node policy contains an invalid capability set",
            Self::IdentityDenied => "judge node is not allowed by deployment policy",
            Self::CapabilityDenied => "judge node declared a capability outside its allowlist",
        })
    }
}

impl Error for NodePolicyError {}

/// Fully server-derived inputs for one P0-C Judge Control claim transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JudgeClaim {
    pub node_id: NodeId,
    pub declared_capabilities: Vec<Capability>,
    pub operation_id: ClaimOperationId,
    pub lease_token: LeaseToken,
    pub now: UnixMillis,
    pub lease_policy: LeasePolicy,
}

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
