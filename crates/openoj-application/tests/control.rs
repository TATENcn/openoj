use std::error::Error;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use openoj_application::{
    CancelEvaluation, ClaimTask, ControlPlane, CreateEvaluation, Decision, EvaluationSnapshot,
    EvaluationStore, RetryExpired, StageContext, StageExecution, StageExecutor, StoreError,
    StoreFuture, SubmitResult, TaskLease,
};
use openoj_domain::{
    AttemptState, Capability, EvaluationState, ExecutorKind, IdempotencyKey, LeaseDuration,
    LeaseToken, NodeId, ResourceUsage, Score, StageKind, UnixMillis, Verdict,
};
use openoj_protocol::decode_evaluation_request;

const VALID_REQUEST: &[u8] =
    include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct DeterministicDevelopmentMock {
    decision: Decision,
}

impl StageExecutor for DeterministicDevelopmentMock {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::DevelopmentMock
    }

    fn production_eligible(&self) -> bool {
        false
    }

    fn node_id(&self) -> Option<NodeId> {
        None
    }

    fn supports(&self, capability: &Capability) -> bool {
        capability.as_str() == "algorithm.batch"
    }

    fn execute(&mut self, context: StageContext<'_>) -> StageExecution {
        StageExecution::Succeeded {
            usage: ResourceUsage::default(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
            decision: (context.stage() == StageKind::Check).then_some(self.decision),
        }
    }
}

struct RecordingStore {
    calls: Arc<Mutex<Vec<&'static str>>>,
    snapshot: EvaluationSnapshot,
    lease: TaskLease,
}

impl RecordingStore {
    fn record(&self, call: &'static str) -> Result<(), StoreError> {
        self.calls
            .lock()
            .map_err(|_| StoreError::Unavailable)?
            .push(call);
        Ok(())
    }
}

impl EvaluationStore for RecordingStore {
    fn create_evaluation(&self, _command: CreateEvaluation) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(async move {
            self.record("create")?;
            Ok(self.snapshot.clone())
        })
    }

    fn evaluation_status(
        &self,
        _evaluation_id: openoj_domain::EvaluationId,
    ) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(async move {
            self.record("status")?;
            Ok(self.snapshot.clone())
        })
    }

    fn claim_task(&self, _command: ClaimTask) -> StoreFuture<'_, TaskLease> {
        Box::pin(async move {
            self.record("claim")?;
            Ok(self.lease.clone())
        })
    }

    fn retry_expired(&self, _command: RetryExpired) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(async move {
            self.record("retry")?;
            Ok(self.snapshot.clone())
        })
    }

    fn submit_result(&self, _command: SubmitResult) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(async move {
            self.record("result")?;
            Ok(self.snapshot.clone())
        })
    }

    fn cancel_evaluation(&self, _command: CancelEvaluation) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(async move {
            self.record("cancel")?;
            Ok(self.snapshot.clone())
        })
    }
}

#[test]
fn control_plane_delegates_each_typed_command() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let result = openoj_application::evaluate(
        &request,
        &mut DeterministicDevelopmentMock {
            decision: Decision::new(Verdict::Accepted, Score::new(1, 1)?)?,
        },
    )?;
    let now = UnixMillis::new(1_000)?;
    let node_id = NodeId::parse("node_01")?;
    let lease_token = LeaseToken::parse("lease_01")?;
    let result_key = IdempotencyKey::parse("result_01")?;
    let snapshot = EvaluationSnapshot {
        evaluation_id: request.evaluation_id().clone(),
        state: EvaluationState::Queued,
        current_attempt_id: request.attempt_id().clone(),
        attempt_number: request.attempt_number(),
        attempt_state: AttemptState::Queued,
        terminal_result: false,
    };
    let lease = TaskLease {
        request: request.clone(),
        node_id: node_id.clone(),
        lease_token: lease_token.clone(),
        expires_at: UnixMillis::new(2_000)?,
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let control = ControlPlane::new(RecordingStore {
        calls: Arc::clone(&calls),
        snapshot: snapshot.clone(),
        lease: lease.clone(),
    });

    assert_eq!(
        block_on(control.create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: now,
        }))?,
        snapshot
    );
    assert_eq!(
        block_on(control.evaluation_status(request.evaluation_id().clone()))?,
        snapshot
    );
    assert_eq!(
        block_on(control.claim_task(ClaimTask {
            node_id,
            lease_token: lease_token.clone(),
            now,
            lease_duration: LeaseDuration::new(1_000)?,
        }))?,
        lease
    );
    assert_eq!(
        block_on(control.retry_expired(RetryExpired {
            request: request.clone(),
            now,
        }))?,
        snapshot
    );
    assert_eq!(
        block_on(control.submit_result(SubmitResult {
            idempotency_key: result_key.clone(),
            lease_token,
            result: result.clone(),
            now,
        }))?,
        snapshot
    );
    assert_eq!(
        block_on(control.cancel_evaluation(CancelEvaluation {
            idempotency_key: result_key,
            evaluation_id: request.evaluation_id().clone(),
            now,
        }))?,
        snapshot
    );

    let recorded = calls.lock().map_err(|_| StoreError::Unavailable)?.clone();
    assert_eq!(
        recorded,
        ["create", "status", "claim", "retry", "result", "cancel"]
    );
    assert_eq!(
        StoreError::Unavailable.to_string(),
        "storage is unavailable"
    );
    Ok(())
}
