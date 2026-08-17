use std::error::Error;

use openoj_application::{LeasePolicy, NodePolicy};
use openoj_control_plane::JudgeControlService;
use openoj_domain::{Capability, LeaseDuration, NodeId};
use openoj_judge_protocol::CLIENT_PROTOCOL_VERSION;
use openoj_judge_protocol::wire::{NegotiateRequest, judge_control_server::JudgeControl};

#[tokio::test]
async fn negotiate_accepts_an_allowlisted_node_and_returns_server_lease_policy()
-> Result<(), Box<dyn Error>> {
    let node_id = NodeId::parse("judge_node_01")?;
    let capability = Capability::parse("algorithm.batch")?;
    let service = JudgeControlService::new(
        NodePolicy::new([(node_id, vec![capability])])?,
        LeasePolicy::new(LeaseDuration::new(30_000)?, LeaseDuration::new(10_000)?)?,
    );

    let response = service
        .negotiate(tonic::Request::new(NegotiateRequest {
            version: CLIENT_PROTOCOL_VERSION.to_owned(),
            node_id: "judge_node_01".to_owned(),
            capabilities: vec!["algorithm.batch".to_owned()],
            client_limits: None,
        }))
        .await?
        .into_inner();

    assert_eq!(response.accepted_version, CLIENT_PROTOCOL_VERSION);
    assert_eq!(response.lease_duration_ms, 30_000);
    assert_eq!(response.renew_after_ms, 10_000);
    Ok(())
}

#[tokio::test]
async fn negotiate_rejects_an_unallowlisted_node() -> Result<(), Box<dyn Error>> {
    let service = JudgeControlService::new(
        NodePolicy::new([(
            NodeId::parse("judge_node_01")?,
            vec![Capability::parse("algorithm.batch")?],
        )])?,
        LeasePolicy::new(LeaseDuration::new(30_000)?, LeaseDuration::new(10_000)?)?,
    );

    let result = service
        .negotiate(tonic::Request::new(NegotiateRequest {
            version: CLIENT_PROTOCOL_VERSION.to_owned(),
            node_id: "judge_node_02".to_owned(),
            capabilities: vec!["algorithm.batch".to_owned()],
            client_limits: None,
        }))
        .await;
    let Err(error) = result else {
        return Err("unallowlisted node unexpectedly negotiated".into());
    };

    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert_eq!(error.message(), "identity_denied");
    Ok(())
}
