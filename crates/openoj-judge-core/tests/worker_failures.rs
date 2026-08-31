//! Transport-neutral worker failure-path regression tests.
//!
//! These cover the P0-C worker state machine's non-happy paths with fake
//! control clients and executors: no-task clamp, claim/execution/submit error
//! propagation, and stale-lease renewal that must never fabricate a result.

use std::error::Error;

use openoj_application::{ApplicationError, JudgeRenewDirective, StoreError, TaskLease};
use openoj_domain::{
    ClaimOperationId, EvaluationRequest, EvaluationResult, LeaseToken, NodeId, ResultOperationId,
    UnixMillis,
};
use openoj_judge_core::{
    AsyncJudgeControlClient, DevelopmentMockExecutor, JudgeControlClient, JudgeExecutor, Worker,
    WorkerClaim, WorkerError, WorkerOutcome,
};
use openoj_protocol::decode_evaluation_request;

const VALID_REQUEST: &[u8] =
    include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

fn decoded_request() -> Result<EvaluationRequest, Box<dyn Error>> {
    Ok(decode_evaluation_request(VALID_REQUEST)?)
}

fn lease(request: EvaluationRequest, node_id: NodeId) -> Result<TaskLease, Box<dyn Error>> {
    Ok(TaskLease {
        request,
        node_id,
        lease_token: LeaseToken::parse("lease_fail_01")?,
        expires_at: UnixMillis::new(30_000)?,
    })
}

/// A deterministic executor that always fails, proving error paths short-circuit
/// before any result is fabricated.
struct FailingExecutor;

impl JudgeExecutor for FailingExecutor {
    fn execute(
        &mut self,
        _request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError> {
        Err(ApplicationError::UnsupportedCapability)
    }
}

/// Sync client that returns a lease and counts submissions.
struct LeaseClient {
    lease: TaskLease,
    submits: u32,
}

impl JudgeControlClient for LeaseClient {
    fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        self.submits += 1;
        Ok(())
    }
}

/// Sync client that always reports an empty queue.
struct NoTaskClient;

impl JudgeControlClient for NoTaskClient {
    fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::NoTask)
    }

    fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Sync client whose claim fails with a transport-level error.
struct ClaimErrorClient;

impl JudgeControlClient for ClaimErrorClient {
    fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Err(StoreError::Unavailable)
    }

    fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Sync client whose submit fails after a successful claim and execution.
struct SubmitErrorClient {
    lease: TaskLease,
}

impl JudgeControlClient for SubmitErrorClient {
    fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }
}

/// Async client that returns a lease but rejects the lease renewal as stale.
struct RenewStaleClient {
    lease: TaskLease,
    submitted: bool,
}

impl AsyncJudgeControlClient for RenewStaleClient {
    fn renewal_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(10)
    }

    async fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    async fn renew(&mut self, _lease: &TaskLease) -> Result<JudgeRenewDirective, StoreError> {
        Err(StoreError::StaleLease)
    }

    async fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        self.submitted = true;
        Ok(())
    }
}

/// Async client that always reports an empty queue.
struct AsyncNoTaskClient {
    renew: JudgeRenewDirective,
    submitted: bool,
}

impl AsyncJudgeControlClient for AsyncNoTaskClient {
    fn renewal_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(10)
    }

    async fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::NoTask)
    }

    async fn renew(&mut self, _lease: &TaskLease) -> Result<JudgeRenewDirective, StoreError> {
        Ok(self.renew)
    }

    async fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        self.submitted = true;
        Ok(())
    }
}

/// Async client whose submit fails after a successful lease and renewal.
struct AsyncSubmitErrorClient {
    lease: TaskLease,
    renew: JudgeRenewDirective,
}

impl AsyncJudgeControlClient for AsyncSubmitErrorClient {
    fn renewal_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(10)
    }

    async fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    async fn renew(&mut self, _lease: &TaskLease) -> Result<JudgeRenewDirective, StoreError> {
        Ok(self.renew)
    }

    async fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: EvaluationResult,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }
}

#[test]
fn worker_returns_no_task_without_executing_or_submitting() -> Result<(), Box<dyn Error>> {
    let mut client = NoTaskClient;
    // A failing executor proves the no-task claim short-circuits before execution.
    let mut worker = Worker::new(FailingExecutor);

    assert_eq!(
        worker.run_once(
            &mut client,
            ClaimOperationId::parse("claim_no_task")?,
            ResultOperationId::parse("result_no_task")?,
        )?,
        WorkerOutcome::NoTask
    );
    Ok(())
}

#[test]
fn worker_propagates_claim_error_without_executing() -> Result<(), Box<dyn Error>> {
    let mut client = ClaimErrorClient;
    let mut worker = Worker::new(FailingExecutor);

    let error = worker.run_once(
        &mut client,
        ClaimOperationId::parse("claim_error")?,
        ResultOperationId::parse("result_error")?,
    );
    assert!(matches!(
        error,
        Err(WorkerError::Store(StoreError::Unavailable))
    ));
    Ok(())
}

#[test]
fn worker_propagates_execution_error_without_submitting() -> Result<(), Box<dyn Error>> {
    let node_id = NodeId::parse("judge_node_01")?;
    let mut client = LeaseClient {
        lease: lease(decoded_request()?, node_id.clone())?,
        submits: 0,
    };
    let mut worker = Worker::new(FailingExecutor);

    let error = worker.run_once(
        &mut client,
        ClaimOperationId::parse("claim_exec")?,
        ResultOperationId::parse("result_exec")?,
    );
    assert!(matches!(
        error,
        Err(WorkerError::Application(
            ApplicationError::UnsupportedCapability
        ))
    ));
    assert_eq!(client.submits, 0);
    Ok(())
}

#[test]
fn worker_propagates_submit_error_after_execution() -> Result<(), Box<dyn Error>> {
    let node_id = NodeId::parse("judge_node_01")?;
    let mut client = SubmitErrorClient {
        lease: lease(decoded_request()?, node_id.clone())?,
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    let error = worker.run_once(
        &mut client,
        ClaimOperationId::parse("claim_submit")?,
        ResultOperationId::parse("result_submit")?,
    );
    assert!(matches!(
        error,
        Err(WorkerError::Store(StoreError::Unavailable))
    ));
    Ok(())
}

#[tokio::test]
async fn async_worker_returns_no_task_without_submitting() -> Result<(), Box<dyn Error>> {
    let mut client = AsyncNoTaskClient {
        renew: JudgeRenewDirective::Continue {
            expires_at: UnixMillis::new(60_000)?,
        },
        submitted: false,
    };
    let mut worker = Worker::new(FailingExecutor);

    assert_eq!(
        worker
            .run_once_async(
                &mut client,
                ClaimOperationId::parse("claim_no_task_async")?,
                ResultOperationId::parse("result_no_task_async")?,
            )
            .await?,
        WorkerOutcome::NoTask
    );
    assert!(!client.submitted);
    Ok(())
}

#[tokio::test]
async fn async_worker_propagates_stale_lease_without_fabricating_a_result()
-> Result<(), Box<dyn Error>> {
    let node_id = NodeId::parse("judge_node_01")?;
    let mut client = RenewStaleClient {
        lease: lease(decoded_request()?, node_id.clone())?,
        submitted: false,
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    let error = worker
        .run_once_async(
            &mut client,
            ClaimOperationId::parse("claim_stale")?,
            ResultOperationId::parse("result_stale")?,
        )
        .await;
    assert!(matches!(
        error,
        Err(WorkerError::Store(StoreError::StaleLease))
    ));
    assert!(!client.submitted);
    Ok(())
}

#[tokio::test]
async fn async_worker_propagates_submit_error_after_renewal() -> Result<(), Box<dyn Error>> {
    let node_id = NodeId::parse("judge_node_01")?;
    let mut client = AsyncSubmitErrorClient {
        lease: lease(decoded_request()?, node_id.clone())?,
        renew: JudgeRenewDirective::Continue {
            expires_at: UnixMillis::new(60_000)?,
        },
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    let error = worker
        .run_once_async(
            &mut client,
            ClaimOperationId::parse("claim_submit_async")?,
            ResultOperationId::parse("result_submit_async")?,
        )
        .await;
    assert!(matches!(
        error,
        Err(WorkerError::Store(StoreError::Unavailable))
    ));
    Ok(())
}
