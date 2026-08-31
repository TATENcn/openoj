use std::env;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use openoj_domain::{Capability, ClaimOperationId, ContentDigest, NodeId, ResultOperationId};
use openoj_firecracker::{
    FirecrackerConfig, FirecrackerConfigParts, MachineConfig, ResourceLimits, VsockConfig,
};
use openoj_judge_core::{Worker, WorkerOutcome};
use openoj_judge_node::{
    AlgorithmCWorkload, FirecrackerExecutor, FirecrackerExecutorConfig, UdsJudgeControlClient,
};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("openoj-judge-node failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let executor_mode =
        env::var("OPENOJ_JUDGE_EXECUTOR").map_err(|_| "executor configuration missing")?;
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

    match executor_mode.as_str() {
        "development_mock" => {
            let worker = Worker::new(openoj_judge_core::DevelopmentMockExecutor::new(node_id));
            run_loop(&mut client, worker).await
        }
        "firecracker" => run_loop(&mut client, worker_firecracker(node_id)?).await,
        _ => Err("executor must be development_mock or firecracker"),
    }
}

async fn run_loop<E: openoj_judge_core::JudgeExecutor + Send + 'static>(
    client: &mut UdsJudgeControlClient,
    mut worker: Worker<E>,
) -> Result<(), &'static str> {
    loop {
        let outcome = worker
            .run_once_async(client, operation_id_claim()?, operation_id_result()?)
            .await
            .map_err(|_| "judge worker iteration failed")?;
        if outcome == WorkerOutcome::NoTask {
            tokio::time::sleep(client.no_task_backoff()).await;
        }
    }
}

fn worker_firecracker(node_id: NodeId) -> Result<Worker<FirecrackerExecutor>, &'static str> {
    let firecracker_path =
        PathBuf::from(env::var("OPENOJ_FC_FIRECRACKER").map_err(|_| "firecracker binary missing")?);
    let api_socket =
        PathBuf::from(env::var("OPENOJ_FC_API_SOCKET").map_err(|_| "api socket missing")?);
    let kernel = PathBuf::from(env::var("OPENOJ_FC_KERNEL").map_err(|_| "kernel missing")?);
    let rootfs = PathBuf::from(env::var("OPENOJ_FC_ROOTFS").map_err(|_| "rootfs missing")?);
    let vsock_socket =
        PathBuf::from(env::var("OPENOJ_FC_VSOCK_SOCKET").map_err(|_| "vsock socket missing")?);
    let production = env::var("OPENOJ_FC_PRODUCTION").as_deref() == Ok("1");
    let jailer_path = env::var("OPENOJ_FC_JAILER").ok().map(PathBuf::from);
    let source_path = PathBuf::from(
        env::var("OPENOJ_FC_SOURCE").map_err(|_| "development source artifact missing")?,
    );
    let workload = AlgorithmCWorkload::new(
        read_development_source(&source_path)?,
        ContentDigest::parse(
            env::var("OPENOJ_FC_RUNTIME_DIGEST").map_err(|_| "runtime digest missing")?,
        )
        .map_err(|_| "runtime digest invalid")?,
        ContentDigest::parse(
            env::var("OPENOJ_FC_EXPECTED_OUTPUT_DIGEST")
                .map_err(|_| "expected output digest missing")?,
        )
        .map_err(|_| "expected output digest invalid")?,
    )
    .map_err(|_| "development workload invalid")?;

    let vm_config = FirecrackerConfig::from_parts(FirecrackerConfigParts {
        kernel_path: kernel,
        kernel_digest: env::var("OPENOJ_FC_KERNEL_DIGEST").map_err(|_| "kernel digest missing")?,
        rootfs_path: rootfs,
        rootfs_digest: env::var("OPENOJ_FC_ROOTFS_DIGEST").map_err(|_| "rootfs digest missing")?,
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off root=/dev/vda ro init=/sbin/openoj-init"
            .to_owned(),
        machine: MachineConfig::new(1, 256).map_err(|_| "machine config invalid")?,
        limits: ResourceLimits::default(),
        vsock: VsockConfig::new(3, 8266).map_err(|_| "vsock config invalid")?,
        vsock_uds_path: vsock_socket,
        jailer_path,
    })
    .map_err(|_| "firecracker config invalid")?;

    let executor = FirecrackerExecutor::try_new(FirecrackerExecutorConfig {
        node_id,
        firecracker_path,
        api_socket,
        vm_config,
        workload,
        production,
    })
    .map_err(|_| "firecracker executor failed to start")?;

    Ok(Worker::new(executor))
}

fn read_development_source(path: &Path) -> Result<Vec<u8>, &'static str> {
    if !path.is_absolute() {
        return Err("development source path must be absolute");
    }
    let file = File::open(path).map_err(|_| "development source artifact unavailable")?;
    let metadata = file
        .metadata()
        .map_err(|_| "development source artifact unavailable")?;
    let maximum = openoj_guest_protocol::MAX_INLINE_BYTES;
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > u64::try_from(maximum).map_err(|_| "source bound invalid")?
    {
        return Err("development source artifact invalid");
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| "development source artifact invalid")?
            .min(maximum),
    );
    file.take(u64::try_from(maximum + 1).map_err(|_| "source bound invalid")?)
        .read_to_end(&mut bytes)
        .map_err(|_| "development source artifact unavailable")?;
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("development source artifact invalid");
    }
    Ok(bytes)
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
