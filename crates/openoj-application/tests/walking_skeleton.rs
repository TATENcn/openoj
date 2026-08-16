use std::error::Error;

use openoj_application::{Decision, StageContext, StageExecution, StageExecutor, evaluate};
use openoj_domain::{Capability, ExecutorKind, NodeId, ResourceUsage, Score, StageKind, Verdict};
use openoj_protocol::{
    decode_evaluation_request, decode_evaluation_result, encode_evaluation_result,
};

const VALID_REQUEST: &[u8] =
    include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

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

#[test]
fn canonical_request_reaches_a_bounded_mock_result() -> Result<(), Box<dyn Error>> {
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let decision = Decision::new(Verdict::Accepted, Score::new(1, 1)?)?;
    let result = evaluate(&request, &mut DeterministicDevelopmentMock { decision })?;
    let encoded = encode_evaluation_result(&result)?;
    let wire_result = decode_evaluation_result(&encoded)?;
    let value = serde_json::to_value(wire_result)?;

    assert_eq!(value["verdict"], "accepted");
    assert_eq!(value["stages"].as_array().map(Vec::len), Some(5));
    assert_eq!(value["provenance"]["executor_kind"], "development_mock");
    assert_eq!(value["provenance"]["production_eligible"], false);
    Ok(())
}
