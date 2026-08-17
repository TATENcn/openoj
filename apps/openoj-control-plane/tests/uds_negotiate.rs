use std::error::Error;
use std::os::unix::fs::PermissionsExt;

use openoj_application::{LeasePolicy, NodePolicy};
use openoj_control_plane::{JudgeControlService, bind_socket, serve_with_shutdown};
use openoj_domain::{Capability, LeaseDuration, NodeId};
use openoj_judge_protocol::CLIENT_PROTOCOL_VERSION;
use openoj_judge_protocol::wire::{NegotiateRequest, judge_control_client::JudgeControlClient};

#[tokio::test]
async fn uds_server_negotiates_without_a_tcp_listener() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let listener = bind_socket(&socket_path).await?;
    let service = JudgeControlService::new(
        NodePolicy::new([(
            NodeId::parse("judge_node_01")?,
            vec![Capability::parse("algorithm.batch")?],
        )])?,
        LeasePolicy::new(LeaseDuration::new(30_000)?, LeaseDuration::new(10_000)?)?,
    );
    let (shutdown, receiver) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_with_shutdown(listener, service, async move {
        let _result = receiver.await;
    }));

    let endpoint = format!("unix://{}", socket_path.display());
    let mut client = JudgeControlClient::connect(endpoint).await?;
    let response = client
        .negotiate(NegotiateRequest {
            version: CLIENT_PROTOCOL_VERSION.to_owned(),
            node_id: "judge_node_01".to_owned(),
            capabilities: vec!["algorithm.batch".to_owned()],
            client_limits: None,
        })
        .await?
        .into_inner();

    assert_eq!(response.accepted_version, CLIENT_PROTOCOL_VERSION);
    shutdown
        .send(())
        .map_err(|()| "server shutdown channel closed")?;
    server.await??;
    Ok(())
}
