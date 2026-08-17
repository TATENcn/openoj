//! Transport-neutral Judge Node worker components.

use openoj_application::{
    ApplicationError, Decision, JudgeRenewDirective, StageContext, StageExecution, StageExecutor,
    StoreError, TaskLease, evaluate,
};
use openoj_domain::{
    Capability, ClaimOperationId, EvaluationRequest, EvaluationResult, ExecutorKind, NodeId,
    ResourceUsage, ResultOperationId, Score, StageKind, Verdict,
};

/// Executes one canonical Evaluation without depending on transport or storage.
pub trait JudgeExecutor {
    /// Executes a complete canonical request.
    ///
    /// # Errors
    ///
    /// Returns a typed evaluation error for unsupported capabilities or invalid output.
    fn execute(
        &mut self,
        request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError>;
}

/// Transport-neutral control operations required by a one-task worker iteration.
pub trait JudgeControlClient {
    /// Claims at most one task for a stable operation identifier.
    ///
    /// # Errors
    ///
    /// Returns a stable control-plane error when the claim cannot be completed.
    fn claim(&mut self, operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError>;

    /// Submits the result associated with a previously claimed lease.
    ///
    /// # Errors
    ///
    /// Returns a stable control-plane error when the result cannot be committed.
    fn submit(
        &mut self,
        lease: &TaskLease,
        operation_id: ResultOperationId,
        result: EvaluationResult,
    ) -> Result<(), StoreError>;
}

/// Async transport-neutral control operations for a Judge Node process.
pub trait AsyncJudgeControlClient: Send {
    /// Claims at most one task for a stable operation identifier.
    ///
    /// # Errors
    ///
    /// Returns a stable control-plane error when the claim cannot be completed.
    fn claim(
        &mut self,
        operation_id: ClaimOperationId,
    ) -> impl Future<Output = Result<WorkerClaim, StoreError>> + Send;

    /// Checks whether a claimed lease remains valid before execution starts.
    ///
    /// # Errors
    ///
    /// Returns a stable control-plane error when the lease cannot be renewed or inspected.
    fn renew(
        &mut self,
        lease: &TaskLease,
    ) -> impl Future<Output = Result<JudgeRenewDirective, StoreError>> + Send;

    /// Submits the result associated with a previously claimed lease.
    ///
    /// # Errors
    ///
    /// Returns a stable control-plane error when the result cannot be committed.
    fn submit(
        &mut self,
        lease: &TaskLease,
        operation_id: ResultOperationId,
        result: EvaluationResult,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerClaim {
    Lease(Box<TaskLease>),
    NoTask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerOutcome {
    Submitted,
    NoTask,
    Cancelled,
}

/// Single-concurrency Judge Node worker.
pub struct Worker<E> {
    executor: E,
}

impl<E> Worker<E> {
    #[must_use]
    pub const fn new(executor: E) -> Self {
        Self { executor }
    }
}

impl<E: JudgeExecutor> Worker<E> {
    /// Claims, executes, and submits at most one task.
    ///
    /// # Errors
    ///
    /// Returns the stable store or execution error without attempting a second claim.
    pub fn run_once<C: JudgeControlClient>(
        &mut self,
        client: &mut C,
        claim_operation_id: ClaimOperationId,
        result_operation_id: ResultOperationId,
    ) -> Result<WorkerOutcome, WorkerError> {
        match client.claim(claim_operation_id)? {
            WorkerClaim::NoTask => Ok(WorkerOutcome::NoTask),
            WorkerClaim::Lease(lease) => {
                let result = self.executor.execute(&lease.request)?;
                client.submit(&lease, result_operation_id, result)?;
                Ok(WorkerOutcome::Submitted)
            }
        }
    }
}

impl<E: JudgeExecutor + Send> Worker<E> {
    /// Claims, executes, and submits at most one task using an async control transport.
    ///
    /// # Errors
    ///
    /// Returns the stable control or execution error without attempting a second claim.
    pub async fn run_once_async<C: AsyncJudgeControlClient>(
        &mut self,
        client: &mut C,
        claim_operation_id: ClaimOperationId,
        result_operation_id: ResultOperationId,
    ) -> Result<WorkerOutcome, WorkerError> {
        match client.claim(claim_operation_id).await? {
            WorkerClaim::NoTask => Ok(WorkerOutcome::NoTask),
            WorkerClaim::Lease(lease) => {
                if client.renew(&lease).await? == JudgeRenewDirective::Cancel {
                    return Ok(WorkerOutcome::Cancelled);
                }
                let result = self.executor.execute(&lease.request)?;
                client.submit(&lease, result_operation_id, result).await?;
                Ok(WorkerOutcome::Submitted)
            }
        }
    }
}

#[derive(Debug)]
pub enum WorkerError {
    Store(StoreError),
    Application(ApplicationError),
}

impl From<StoreError> for WorkerError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<ApplicationError> for WorkerError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "judge control operation failed: {error}"),
            Self::Application(error) => write!(formatter, "judge execution failed: {error}"),
        }
    }
}

impl std::error::Error for WorkerError {}

/// Deterministic P0-C development executor that never launches processes or reads task files.
pub struct DevelopmentMockExecutor {
    node_id: NodeId,
}

impl DevelopmentMockExecutor {
    #[must_use]
    pub const fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }
}

impl JudgeExecutor for DevelopmentMockExecutor {
    fn execute(
        &mut self,
        request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError> {
        evaluate(request, self)
    }
}

impl StageExecutor for DevelopmentMockExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::DevelopmentMock
    }

    fn production_eligible(&self) -> bool {
        false
    }

    fn node_id(&self) -> Option<NodeId> {
        Some(self.node_id.clone())
    }

    fn supports(&self, capability: &Capability) -> bool {
        capability.as_str() == "algorithm.batch"
    }

    fn execute(&mut self, context: StageContext<'_>) -> StageExecution {
        let decision = match Score::new(1, 1) {
            Ok(score) => Decision::new(Verdict::Accepted, score).ok(),
            Err(_) => None,
        };
        StageExecution::Succeeded {
            usage: ResourceUsage::default(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
            decision: (context.stage() == StageKind::Check)
                .then_some(decision)
                .flatten(),
        }
    }
}
