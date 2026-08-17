use std::error::Error;

use openoj_domain::{NodeId, Verdict};
use openoj_judge_core::{DevelopmentMockExecutor, JudgeExecutor};
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
