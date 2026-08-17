use std::env;
use std::path::PathBuf;

use openoj_domain::{Capability, ClaimOperationId, NodeId, ResultOperationId};
use openoj_judge_core::{DevelopmentMockExecutor, Worker, WorkerOutcome};
use openoj_judge_node::UdsJudgeControlClient;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("openoj-judge-node failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    if env::var("OPENOJ_JUDGE_EXECUTOR").as_deref() != Ok("development_mock") {
        return Err("only explicit development_mock executor is supported in P0-C");
    }
    let socket_path = PathBuf::from(
        env::var("OPENOJ_JUDGE_CONTROL_SOCKET").map_err(|_| "socket configuration missing")?,
    );
    let node_id = NodeId::parse(
        env::var("OPENOJ_JUDGE_NODE_ID").map_err(|_| "node identity configuration missing")?,
    )
    .map_err(|_| "node identity configuration invalid")?;
    let capabilities =
        vec![Capability::parse("algorithm.batch").map_err(|_| "capability invalid")?];
    let mut client = UdsJudgeControlClient::connect(&socket_path, node_id.clone(), capabilities)
        .await
        .map_err(|_| "Judge Control negotiation failed")?;
    let mut worker = Worker::new(DevelopmentMockExecutor::new(node_id));
    loop {
        let outcome = worker
            .run_once_async(&mut client, operation_id_claim()?, operation_id_result()?)
            .await
            .map_err(|_| "judge worker iteration failed")?;
        if outcome == WorkerOutcome::NoTask {
            tokio::time::sleep(client.no_task_backoff()).await;
        }
    }
}

fn operation_id_claim() -> Result<ClaimOperationId, &'static str> {
    ClaimOperationId::parse(operation_id("claim")?).map_err(|_| "claim operation generation failed")
}

fn operation_id_result() -> Result<ResultOperationId, &'static str> {
    ResultOperationId::parse(operation_id("result")?)
        .map_err(|_| "result operation generation failed")
}

fn operation_id(prefix: &str) -> Result<String, &'static str> {
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| "operation random source unavailable")?;
    Ok(format!("{prefix}_{}", hex::encode(bytes)))
}
