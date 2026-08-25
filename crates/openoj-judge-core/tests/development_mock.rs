use std::error::Error;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use openoj_application::{ApplicationError, JudgeRenewDirective, StoreError, TaskLease};
use openoj_domain::{
    ClaimOperationId, EvaluationRequest, EvaluationResult, NodeId, ResultOperationId, Verdict,
};
use openoj_judge_core::{
    AsyncJudgeControlClient, DevelopmentMockExecutor, ExecutionCancellation, JudgeControlClient,
    JudgeExecutor, Worker, WorkerClaim, WorkerOutcome,
};
use openoj_protocol::decode_evaluation_request;

const VALID_REQUEST: &[u8] =
    include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

#[test]
fn development_mock_returns_a_non_production_result_with_node_provenance()
-> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let mut executor = DevelopmentMockExecutor::new(node_id.clone());

    let result = executor.execute(&request)?;

    assert_eq!(result.verdict(), Verdict::Accepted);
    assert!(!result.provenance().production_eligible());
    assert_eq!(result.provenance().node_id(), Some(&node_id));
    Ok(())
}

struct FakeClient {
    lease: TaskLease,
    submitted: bool,
}

struct FakeAsyncClient {
    lease: TaskLease,
    directive: JudgeRenewDirective,
    submitted: bool,
}

struct CancelDuringExecutionClient {
    lease: TaskLease,
    renewals: u32,
    submitted: bool,
    stop: RenewalStop,
}

#[derive(Clone, Copy)]
enum RenewalStop {
    Cancel,
    StaleLease,
}

impl AsyncJudgeControlClient for CancelDuringExecutionClient {
    fn renewal_interval(&self) -> Duration {
        Duration::from_millis(10)
    }

    async fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    async fn renew(&mut self, _lease: &TaskLease) -> Result<JudgeRenewDirective, StoreError> {
        self.renewals += 1;
        if self.renewals == 1 {
            Ok(JudgeRenewDirective::Continue {
                expires_at: openoj_domain::UnixMillis::new(60_000)
                    .map_err(|_| StoreError::InvalidTime)?,
            })
        } else {
            match self.stop {
                RenewalStop::Cancel => Ok(JudgeRenewDirective::Cancel),
                RenewalStop::StaleLease => Err(StoreError::StaleLease),
            }
        }
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

#[derive(Default)]
struct BlockingState {
    cancelled: Mutex<bool>,
    wake: Condvar,
}

struct BlockingExecutor {
    state: Arc<BlockingState>,
}

struct BlockingCancellation {
    state: Arc<BlockingState>,
}

impl ExecutionCancellation for BlockingCancellation {
    fn cancel(&self) -> Result<(), ApplicationError> {
        let mut cancelled =
            self.state
                .cancelled
                .lock()
                .map_err(|_| ApplicationError::InvalidExecutorOutput {
                    reason: "test cancellation lock unavailable",
                })?;
        *cancelled = true;
        self.state.wake.notify_all();
        Ok(())
    }
}

impl JudgeExecutor for BlockingExecutor {
    fn execute(
        &mut self,
        _request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError> {
        let cancelled =
            self.state
                .cancelled
                .lock()
                .map_err(|_| ApplicationError::InvalidExecutorOutput {
                    reason: "test execution lock unavailable",
                })?;
        let (cancelled, timeout) = self
            .state
            .wake
            .wait_timeout_while(cancelled, Duration::from_secs(1), |cancelled| !*cancelled)
            .map_err(|_| ApplicationError::InvalidExecutorOutput {
                reason: "test execution wait unavailable",
            })?;
        if timeout.timed_out() || !*cancelled {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "test executor was not cancelled",
            });
        }
        Err(ApplicationError::InvalidExecutorOutput {
            reason: "test executor cancelled",
        })
    }

    fn cancellation_handle(&self) -> Box<dyn ExecutionCancellation> {
        Box::new(BlockingCancellation {
            state: Arc::clone(&self.state),
        })
    }
}

impl AsyncJudgeControlClient for FakeAsyncClient {
    fn renewal_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(10)
    }

    async fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    async fn renew(&mut self, _lease: &TaskLease) -> Result<JudgeRenewDirective, StoreError> {
        Ok(self.directive)
    }

    async fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: openoj_domain::EvaluationResult,
    ) -> Result<(), StoreError> {
        self.submitted = true;
        Ok(())
    }
}

impl JudgeControlClient for FakeClient {
    fn claim(&mut self, _operation_id: ClaimOperationId) -> Result<WorkerClaim, StoreError> {
        Ok(WorkerClaim::Lease(Box::new(self.lease.clone())))
    }

    fn submit(
        &mut self,
        _lease: &TaskLease,
        _operation_id: ResultOperationId,
        _result: openoj_domain::EvaluationResult,
    ) -> Result<(), StoreError> {
        self.submitted = true;
        Ok(())
    }
}

#[test]
fn worker_claims_executes_and_submits_one_development_mock_task() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let lease = TaskLease {
        request,
        node_id: node_id.clone(),
        lease_token: openoj_domain::LeaseToken::parse("lease_01")?,
        expires_at: openoj_domain::UnixMillis::new(30_000)?,
    };
    let mut client = FakeClient {
        lease,
        submitted: false,
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    assert_eq!(
        worker.run_once(
            &mut client,
            ClaimOperationId::parse("claim_01")?,
            ResultOperationId::parse("result_01")?,
        )?,
        WorkerOutcome::Submitted
    );
    assert!(client.submitted);
    Ok(())
}

#[tokio::test]
async fn async_worker_interrupts_execution_when_the_lease_is_lost() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let lease = TaskLease {
        request,
        node_id,
        lease_token: openoj_domain::LeaseToken::parse("lease_lost_running")?,
        expires_at: openoj_domain::UnixMillis::new(30_000)?,
    };
    let state = Arc::new(BlockingState::default());
    let mut client = CancelDuringExecutionClient {
        lease,
        renewals: 0,
        submitted: false,
        stop: RenewalStop::StaleLease,
    };
    let mut worker = Worker::new(BlockingExecutor {
        state: Arc::clone(&state),
    });

    let outcome = worker
        .run_once_async(
            &mut client,
            ClaimOperationId::parse("claim_lost_running")?,
            ResultOperationId::parse("result_lost_running")?,
        )
        .await;
    assert!(matches!(
        outcome,
        Err(openoj_judge_core::WorkerError::Store(
            StoreError::StaleLease
        ))
    ));
    assert_eq!(client.renewals, 2);
    assert!(!client.submitted);
    assert!(*state.cancelled.lock().map_err(|_| "test lock poisoned")?);
    Ok(())
}

#[tokio::test]
async fn async_worker_checks_renewal_before_submitting() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let lease = TaskLease {
        request,
        node_id: node_id.clone(),
        lease_token: openoj_domain::LeaseToken::parse("lease_renew_01")?,
        expires_at: openoj_domain::UnixMillis::new(30_000)?,
    };
    let mut client = FakeAsyncClient {
        lease,
        directive: JudgeRenewDirective::Continue {
            expires_at: openoj_domain::UnixMillis::new(60_000)?,
        },
        submitted: false,
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    assert_eq!(
        worker
            .run_once_async(
                &mut client,
                ClaimOperationId::parse("claim_renew_01")?,
                ResultOperationId::parse("result_renew_01")?,
            )
            .await?,
        WorkerOutcome::Submitted
    );
    assert!(client.submitted);
    Ok(())
}

#[tokio::test]
async fn async_worker_does_not_fabricate_a_result_after_cancel() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let lease = TaskLease {
        request,
        node_id: node_id.clone(),
        lease_token: openoj_domain::LeaseToken::parse("lease_cancel_01")?,
        expires_at: openoj_domain::UnixMillis::new(30_000)?,
    };
    let mut client = FakeAsyncClient {
        lease,
        directive: JudgeRenewDirective::Cancel,
        submitted: false,
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    assert_eq!(
        worker
            .run_once_async(
                &mut client,
                ClaimOperationId::parse("claim_cancel_01")?,
                ResultOperationId::parse("result_cancel_01")?,
            )
            .await?,
        WorkerOutcome::Cancelled
    );
    assert!(!client.submitted);
    Ok(())
}

#[tokio::test]
async fn async_worker_checks_a_final_renewal_before_submit() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let lease = TaskLease {
        request,
        node_id: node_id.clone(),
        lease_token: openoj_domain::LeaseToken::parse("lease_cancel_before_submit")?,
        expires_at: openoj_domain::UnixMillis::new(30_000)?,
    };
    let mut client = CancelDuringExecutionClient {
        lease,
        renewals: 0,
        submitted: false,
        stop: RenewalStop::Cancel,
    };
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));

    assert_eq!(
        worker
            .run_once_async(
                &mut client,
                ClaimOperationId::parse("claim_cancel_before_submit")?,
                ResultOperationId::parse("result_cancel_before_submit")?,
            )
            .await?,
        WorkerOutcome::Cancelled
    );
    assert_eq!(client.renewals, 2);
    assert!(!client.submitted);
    Ok(())
}

#[tokio::test]
async fn async_worker_interrupts_execution_when_a_renewal_cancels() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let node_id = NodeId::parse("judge_node_01")?;
    let lease = TaskLease {
        request,
        node_id,
        lease_token: openoj_domain::LeaseToken::parse("lease_cancel_running")?,
        expires_at: openoj_domain::UnixMillis::new(30_000)?,
    };
    let state = Arc::new(BlockingState::default());
    let mut client = CancelDuringExecutionClient {
        lease,
        renewals: 0,
        submitted: false,
        stop: RenewalStop::Cancel,
    };
    let mut worker = Worker::new(BlockingExecutor {
        state: Arc::clone(&state),
    });

    assert_eq!(
        worker
            .run_once_async(
                &mut client,
                ClaimOperationId::parse("claim_cancel_running")?,
                ResultOperationId::parse("result_cancel_running")?,
            )
            .await?,
        WorkerOutcome::Cancelled
    );
    assert_eq!(client.renewals, 2);
    assert!(!client.submitted);
    assert!(*state.cancelled.lock().map_err(|_| "test lock poisoned")?);
    Ok(())
}
