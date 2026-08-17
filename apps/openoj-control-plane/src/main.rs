use std::env;
use std::path::PathBuf;

use openoj_application::{LeasePolicy, NodePolicy};
use openoj_control_plane::{JudgeControlService, bind_socket, serve_with_shutdown};
use openoj_domain::{Capability, LeaseDuration, NodeId};
use openoj_storage::{DatabasePoolSize, PostgresEvaluationStore};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("openoj-control-plane failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let database_url =
        env::var("OPENOJ_DATABASE_URL").map_err(|_| "database configuration missing")?;
    let socket_path = PathBuf::from(
        env::var("OPENOJ_JUDGE_CONTROL_SOCKET").map_err(|_| "socket configuration missing")?,
    );
    let node_policy = parse_node_policy(
        &env::var("OPENOJ_JUDGE_NODES").map_err(|_| "node policy configuration missing")?,
    )?;
    let lease_policy = LeasePolicy::new(
        LeaseDuration::new(30_000).map_err(|_| "lease configuration invalid")?,
        LeaseDuration::new(10_000).map_err(|_| "lease configuration invalid")?,
    )
    .map_err(|_| "lease configuration invalid")?;
    let pool_size = DatabasePoolSize::new(4).ok_or("database pool configuration invalid")?;
    let store = PostgresEvaluationStore::connect(&database_url, pool_size)
        .await
        .map_err(|_| "database unavailable")?;
    store
        .migrate()
        .await
        .map_err(|_| "database migration failed")?;
    store
        .check_compatibility()
        .await
        .map_err(|_| "database schema incompatible")?;
    let listener = bind_socket(socket_path)
        .await
        .map_err(|_| "socket setup failed")?;
    let service = JudgeControlService::new(node_policy, lease_policy).with_store(store);
    serve_with_shutdown(listener, service, async {
        let _result = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|_| "UDS service failed")
}

fn parse_node_policy(value: &str) -> Result<NodePolicy, &'static str> {
    let nodes = value
        .split(';')
        .map(|entry| -> Result<(NodeId, Vec<Capability>), &'static str> {
            let (node, capabilities) = entry.split_once(':').ok_or("node policy invalid")?;
            let node = NodeId::parse(node).map_err(|_| "node policy invalid")?;
            let capabilities = capabilities
                .split(',')
                .map(|capability| Capability::parse(capability).map_err(|_| "node policy invalid"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((node, capabilities))
        })
        .collect::<Result<Vec<_>, _>>()?;
    NodePolicy::new(nodes).map_err(|_| "node policy invalid")
}
