use std::error::Error;

use openoj_application::{StoreError, TaskLease};
use openoj_domain::{ClaimOperationId, NodeId, ResultOperationId, Verdict};
use openoj_judge_core::{
    DevelopmentMockExecutor, JudgeControlClient, JudgeExecutor, Worker, WorkerClaim, WorkerOutcome,
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
