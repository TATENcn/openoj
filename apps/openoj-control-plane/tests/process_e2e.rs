use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use openoj_application::{
    CancelEvaluation, ControlPlane, StageContext, StageExecution, StageExecutor,
};
use openoj_domain::{
    AttemptState, Capability, EvaluationState, ExecutorKind, IdempotencyKey, NodeId, ResourceUsage,
    StageKind, StageStatus, UnixMillis, Verdict,
};
use openoj_protocol::{decode_evaluation_request, decode_evaluation_result_domain};
use openoj_storage::{DatabasePoolSize, PostgresEvaluationStore};

const VALID_REQUEST: &str =
    include_str!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");
const ALGORITHM_C_SOURCE: &[u8] =
    b"#include <stdio.h>\nint main(void) { puts(\"42\"); return 0; }\n";
const ALGORITHM_C_SOURCE_SHA256: &str =
    "c2329bffd207edd619d4ca7c7cdd872b76374e1a50421f705020671ca1f85556";
const ALGORITHM_C_OUTPUT_SHA256: &str =
    "084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0";
const ALGORITHM_C_OUTPUT_CONTENT_DIGEST: &str =
    "sha256:084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0";
const ALGORITHM_C_COMPILE_ERROR_SOURCE: &[u8] = b"int main(void) { this is not C; }\n";
const ALGORITHM_C_COMPILE_ERROR_SOURCE_SHA256: &str =
    "45c508a05870369dd65fb951ee804127925b454fa0078ac6e37f84f5a1e3fa06";
const ALGORITHM_C_TIMEOUT_SOURCE: &[u8] = b"int main(void) { for (;;) {} }\n";
const ALGORITHM_C_TIMEOUT_SOURCE_SHA256: &str =
    "84950edbf9514ebef845181f9f296bf2a71be3032f0d1d27b28b93fe1fd57f54";

#[test]
fn postgres_uds_and_two_child_processes_reach_a_terminal_mock_result() -> Result<(), Box<dyn Error>>
{
    let test_db = FreshDatabase::new()?;
    let database_url = test_db.url().to_owned();
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let request_path = directory.path().join("request.json");
    let evaluation_id = unique_request(&request_path)?;
    let root = workspace_root()?;

    let mut control_plane = ChildGuard::spawn(
        Command::new(env!("CARGO_BIN_EXE_openoj-control-plane"))
            .env("OPENOJ_DATABASE_URL", &database_url)
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
            .env("OPENOJ_JUDGE_NODES", "judge_node_01:algorithm.batch")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )?;
    wait_for_socket(&socket_path)?;
    let mut judge_node = ChildGuard::spawn(
        cargo_process(&root, "openoj-judge-node")
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
            .env("OPENOJ_JUDGE_NODE_ID", "judge_node_01")
            .env("OPENOJ_JUDGE_EXECUTOR", "development_mock")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )?;

    let submit = cargo_process(&root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", &database_url)
        .arg("submit")
        .arg(&request_path)
        .output()?;
    if !submit.status.success() {
        return Err(format!(
            "CLI submit process failed: {}",
            String::from_utf8_lossy(&submit.stderr)
        )
        .into());
    }

    wait_for_terminal_status(&root, &database_url, &evaluation_id)?;
    judge_node.stop();
    control_plane.stop();
    fs::remove_file(socket_path)?;
    Ok(())
}

#[test]
fn postgres_uds_and_judge_node_reach_a_persisted_real_microvm_result() -> Result<(), Box<dyn Error>>
{
    run_real_microvm_case(RealMicrovmCase {
        source: ALGORITHM_C_SOURCE,
        source_sha256: ALGORITHM_C_SOURCE_SHA256,
        expected_verdict: Verdict::Accepted,
        failed_stage: None,
        wall_time_ms: 2_000,
    })
}

#[test]
fn real_microvm_compile_error_is_persisted_and_reclaimed() -> Result<(), Box<dyn Error>> {
    run_real_microvm_case(RealMicrovmCase {
        source: ALGORITHM_C_COMPILE_ERROR_SOURCE,
        source_sha256: ALGORITHM_C_COMPILE_ERROR_SOURCE_SHA256,
        expected_verdict: Verdict::CompileError,
        failed_stage: Some(StageKind::Build),
        wall_time_ms: 2_000,
    })
}

#[test]
fn real_microvm_timeout_is_persisted_and_reclaimed() -> Result<(), Box<dyn Error>> {
    run_real_microvm_case(RealMicrovmCase {
        source: ALGORITHM_C_TIMEOUT_SOURCE,
        source_sha256: ALGORITHM_C_TIMEOUT_SOURCE_SHA256,
        expected_verdict: Verdict::TimeLimitExceeded,
        failed_stage: Some(StageKind::Run),
        wall_time_ms: 2_000,
    })
}

#[test]
fn real_microvm_cancellation_wins_late_result_and_reclaims() -> Result<(), Box<dyn Error>> {
    let Some(images) = load_runtime_images()? else {
        return unavailable("algorithm-c runtime images are not provisioned");
    };
    if !kvm_accessible() {
        return unavailable("/dev/kvm is absent or not read-write accessible");
    }
    let firecracker = firecracker_path();
    if !firecracker_available(&firecracker) {
        return unavailable("Firecracker is absent or not executable");
    }

    let test_db = FreshDatabase::new()?;
    let database_url = test_db.url().to_owned();
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let api_socket = directory.path().join("firecracker.sock");
    let vsock_socket = directory.path().join("vsock.sock");
    let source_path = directory.path().join("main.c");
    fs::write(&source_path, ALGORITHM_C_TIMEOUT_SOURCE)?;
    let request_path = directory.path().join("request.json");
    let evaluation_id = unique_firecracker_request(
        &request_path,
        &format!("sha256:{}", images.runtime_sha256),
        ALGORITHM_C_TIMEOUT_SOURCE_SHA256,
        ALGORITHM_C_TIMEOUT_SOURCE.len(),
        8_000,
    )?;
    let request = decode_evaluation_request(&fs::read(&request_path)?)?;
    let root = workspace_root()?;

    let mut control_plane = spawn_real_e2e_control_plane(&database_url, &socket_path)?;
    wait_for_socket(&socket_path)?;
    submit_request(&root, &database_url, &request_path)?;
    let mut judge_node = spawn_real_e2e_judge_node(
        &root,
        &socket_path,
        &api_socket,
        &vsock_socket,
        &source_path,
        &firecracker,
        &images,
    )?;

    wait_for_path(&api_socket, "Firecracker API socket")?;
    thread::sleep(Duration::from_secs(2));

    let runtime = tokio::runtime::Runtime::new()?;
    let store = runtime.block_on(PostgresEvaluationStore::connect(
        &database_url,
        DatabasePoolSize::new(4).ok_or("invalid test database pool size")?,
    ))?;
    let control = ControlPlane::new(store);
    let mut cancellation_executor = CancellationResultExecutor;
    let result = openoj_application::evaluate(&request, &mut cancellation_executor)?;
    let command = CancelEvaluation {
        idempotency_key: IdempotencyKey::parse("cancel_real_microvm_run")?,
        evaluation_id: request.evaluation_id().clone(),
        result,
        now: current_unix_millis()?,
    };
    let cancelled = runtime.block_on(control.cancel_evaluation(command.clone()))?;
    let replayed = runtime.block_on(control.cancel_evaluation(command))?;
    assert_eq!(cancelled, replayed);
    assert_eq!(cancelled.state, EvaluationState::Cancelled);
    assert_eq!(cancelled.attempt_state, AttemptState::Cancelled);

    let node_status = judge_node.wait_for_exit(Duration::from_secs(15))?;
    assert!(
        !node_status.success(),
        "judge node must reject its late result after cancellation"
    );
    assert_cancelled_status(&root, &database_url, &evaluation_id)?;
    let persisted = persisted_result(&database_url, &evaluation_id)?;
    assert_cancelled_microvm_result(&persisted)?;

    control_plane.stop();
    assert!(
        !api_socket.exists(),
        "Firecracker API socket was not reclaimed"
    );
    assert!(
        !vsock_socket.exists(),
        "Firecracker vsock path was not reclaimed"
    );
    fs::remove_file(socket_path)?;
    Ok(())
}

#[derive(Clone, Copy)]
struct RealMicrovmCase {
    source: &'static [u8],
    source_sha256: &'static str,
    expected_verdict: Verdict,
    failed_stage: Option<StageKind>,
    wall_time_ms: u64,
}

fn run_real_microvm_case(case: RealMicrovmCase) -> Result<(), Box<dyn Error>> {
    let Some(images) = load_runtime_images()? else {
        return unavailable("algorithm-c runtime images are not provisioned");
    };
    if !kvm_accessible() {
        return unavailable("/dev/kvm is absent or not read-write accessible");
    }
    let firecracker = firecracker_path();
    if !firecracker_available(&firecracker) {
        return unavailable("Firecracker is absent or not executable");
    }

    let test_db = FreshDatabase::new()?;
    let database_url = test_db.url().to_owned();
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let api_socket = directory.path().join("firecracker.sock");
    let vsock_socket = directory.path().join("vsock.sock");
    let source_path = directory.path().join("main.c");
    fs::write(&source_path, case.source)?;
    let request_path = directory.path().join("request.json");
    let evaluation_id = unique_firecracker_request(
        &request_path,
        &format!("sha256:{}", images.runtime_sha256),
        case.source_sha256,
        case.source.len(),
        case.wall_time_ms,
    )?;
    let root = workspace_root()?;

    let mut control_plane = spawn_real_e2e_control_plane(&database_url, &socket_path)?;
    wait_for_socket(&socket_path)?;

    submit_request(&root, &database_url, &request_path)?;

    let mut judge_node = spawn_real_e2e_judge_node(
        &root,
        &socket_path,
        &api_socket,
        &vsock_socket,
        &source_path,
        &firecracker,
        &images,
    )?;

    wait_for_any_terminal_status(&root, &database_url, &evaluation_id)?;
    let result = persisted_result(&database_url, &evaluation_id)?;
    assert_real_microvm_result(&result, case)?;

    judge_node.stop();
    control_plane.stop();
    assert!(
        !api_socket.exists(),
        "Firecracker API socket was not reclaimed"
    );
    assert!(
        !vsock_socket.exists(),
        "Firecracker vsock path was not reclaimed"
    );
    fs::remove_file(socket_path)?;
    Ok(())
}

fn assert_real_microvm_result(
    result: &openoj_domain::EvaluationResult,
    case: RealMicrovmCase,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        result.verdict(),
        case.expected_verdict,
        "persisted result: {result:?}"
    );
    assert_eq!(
        result.provenance().executor_kind(),
        ExecutorKind::Firecracker
    );
    assert!(!result.provenance().production_eligible());
    assert_eq!(
        result
            .provenance()
            .node_id()
            .map(openoj_domain::NodeId::as_str),
        Some("judge_node_fc_01")
    );
    if let Some(failed_stage) = case.failed_stage {
        let failed_index = result
            .stages()
            .iter()
            .position(|stage| stage.stage() == failed_stage)
            .ok_or("expected failed stage missing")?;
        let failed = &result.stages()[failed_index];
        assert_eq!(failed.status(), StageStatus::Failed);
        assert!(
            !failed.diagnostics().is_empty(),
            "failed stage must retain bounded diagnostics"
        );
        assert!(
            failed
                .evidence()
                .first()
                .and_then(|evidence| evidence.artifact())
                .is_some(),
            "failed stage must retain content-addressed evidence"
        );
        assert!(
            result.stages()[failed_index + 1..]
                .iter()
                .all(|stage| stage.status() == StageStatus::Skipped),
            "stages after the terminal failure must be skipped"
        );
    } else {
        let run = result
            .stages()
            .iter()
            .find(|stage| stage.stage() == StageKind::Run)
            .ok_or("run stage missing")?;
        assert_eq!(
            run.evidence()
                .first()
                .and_then(|evidence| evidence.artifact())
                .map(|artifact| artifact.digest().as_str()),
            Some(ALGORITHM_C_OUTPUT_CONTENT_DIGEST)
        );
    }
    Ok(())
}

struct CancellationResultExecutor;

impl StageExecutor for CancellationResultExecutor {
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

    fn execute(&mut self, _context: StageContext<'_>) -> StageExecution {
        StageExecution::Cancelled {
            usage: ResourceUsage::default(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

fn assert_cancelled_microvm_result(
    result: &openoj_domain::EvaluationResult,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(result.verdict(), Verdict::Cancelled);
    assert_eq!(
        result.provenance().executor_kind(),
        ExecutorKind::DevelopmentMock
    );
    assert!(!result.provenance().production_eligible());
    assert!(result.provenance().node_id().is_none());
    let cancelled_index = result
        .stages()
        .iter()
        .position(|stage| stage.status() == StageStatus::Cancelled)
        .ok_or("cancelled stage missing")?;
    assert!(
        result.stages()[cancelled_index + 1..]
            .iter()
            .all(|stage| stage.status() == StageStatus::Skipped),
        "stages after cancellation must be skipped"
    );
    Ok(())
}

fn submit_request(
    root: &Path,
    database_url: &str,
    request_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let submit = cargo_process(root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", database_url)
        .arg("submit")
        .arg(request_path)
        .output()?;
    if !submit.status.success() {
        return Err(format!(
            "CLI submit process failed: {}",
            String::from_utf8_lossy(&submit.stderr)
        )
        .into());
    }
    Ok(())
}

fn spawn_real_e2e_control_plane(
    database_url: &str,
    socket_path: &Path,
) -> Result<ChildGuard, Box<dyn Error>> {
    ChildGuard::spawn(
        Command::new(env!("CARGO_BIN_EXE_openoj-control-plane"))
            .env("OPENOJ_DATABASE_URL", database_url)
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", socket_path)
            .env("OPENOJ_JUDGE_NODES", "judge_node_fc_01:algorithm.batch")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
}

fn spawn_real_e2e_judge_node(
    root: &Path,
    socket_path: &Path,
    api_socket: &Path,
    vsock_socket: &Path,
    source_path: &Path,
    firecracker: &Path,
    images: &RuntimeImages,
) -> Result<ChildGuard, Box<dyn Error>> {
    ChildGuard::spawn(
        cargo_process(root, "openoj-judge-node")
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", socket_path)
            .env("OPENOJ_JUDGE_NODE_ID", "judge_node_fc_01")
            .env("OPENOJ_JUDGE_EXECUTOR", "firecracker")
            .env("OPENOJ_FC_FIRECRACKER", firecracker)
            .env("OPENOJ_FC_API_SOCKET", api_socket)
            .env("OPENOJ_FC_VSOCK_SOCKET", vsock_socket)
            .env("OPENOJ_FC_KERNEL", &images.kernel)
            .env(
                "OPENOJ_FC_KERNEL_DIGEST",
                format!("sha256:{}", images.kernel_sha256),
            )
            .env("OPENOJ_FC_ROOTFS", &images.rootfs)
            .env(
                "OPENOJ_FC_ROOTFS_DIGEST",
                format!("sha256:{}", images.rootfs_sha256),
            )
            .env("OPENOJ_FC_SOURCE", source_path)
            .env(
                "OPENOJ_FC_RUNTIME_DIGEST",
                format!("sha256:{}", images.runtime_sha256),
            )
            .env(
                "OPENOJ_FC_EXPECTED_OUTPUT_DIGEST",
                format!("sha256:{ALGORITHM_C_OUTPUT_SHA256}"),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
}

struct RuntimeImages {
    kernel: PathBuf,
    kernel_sha256: String,
    rootfs: PathBuf,
    rootfs_sha256: String,
    runtime_sha256: String,
}

fn strict_kvm() -> bool {
    env::var("OPENOJ_REQUIRE_KVM").as_deref() == Ok("1")
}

fn unavailable(reason: &str) -> Result<(), Box<dyn Error>> {
    if strict_kvm() {
        return Err(format!("strict real-microVM E2E requirement not met: {reason}").into());
    }
    println!("skipping real-microVM process E2E: {reason}");
    Ok(())
}

fn kvm_accessible() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok()
}

fn firecracker_path() -> PathBuf {
    env::var_os("OPENOJ_FC_FIRECRACKER").map_or_else(|| PathBuf::from("firecracker"), PathBuf::from)
}

fn firecracker_available(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn load_runtime_images() -> Result<Option<RuntimeImages>, Box<dyn Error>> {
    let root = env::var_os("OPENOJ_FC_TEST_IMAGES").map_or_else(
        || workspace_root().map(|root| root.join("infra/runtime-images/algorithm-c/out")),
        |path| Ok(PathBuf::from(path)),
    )?;
    let kernel = root.join("kernel/vmlinux.bin");
    let rootfs = root.join("rootfs/rootfs.ext4");
    let manifest = root.join("manifest.json");
    let checksums = root.join("manifest.sha256");
    if !kernel.is_file() || !rootfs.is_file() || !manifest.is_file() || !checksums.is_file() {
        return Ok(None);
    }
    let verified = Command::new("sha256sum")
        .args(["--check", "--strict", "manifest.sha256"])
        .current_dir(&root)
        .output()?;
    if !verified.status.success() {
        return Err("runtime image checksum verification failed".into());
    }
    Ok(Some(RuntimeImages {
        kernel_sha256: sha256_file(&kernel)?,
        rootfs_sha256: sha256_file(&rootfs)?,
        runtime_sha256: sha256_file(&manifest)?,
        kernel,
        rootfs,
    }))
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("sha256sum").arg(path).output()?;
    if !output.status.success() {
        return Err("artifact digest calculation failed".into());
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "artifact digest output malformed".into())
}

/// A judge node whose declared capability is not in its deployment allowlist must
/// fail closed during negotiation and leave the submitted task queued, never
/// reaching a terminal state on the host.
#[test]
fn unmatched_capability_judge_node_fails_closed_and_leaves_task_queued()
-> Result<(), Box<dyn Error>> {
    let test_db = FreshDatabase::new()?;
    let database_url = test_db.url().to_owned();
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let request_path = directory.path().join("request.json");
    let evaluation_id = unique_request(&request_path)?;
    let root = workspace_root()?;

    // The allowlist authorizes the node only for a capability the task does not use,
    // so the node's declared `algorithm.batch` is denied.
    let mut control_plane = ChildGuard::spawn(
        Command::new(env!("CARGO_BIN_EXE_openoj-control-plane"))
            .env("OPENOJ_DATABASE_URL", &database_url)
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
            .env("OPENOJ_JUDGE_NODES", "judge_node_01:algorithm.grading")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )?;
    wait_for_socket(&socket_path)?;

    let submit = cargo_process(&root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", &database_url)
        .arg("submit")
        .arg(&request_path)
        .output()?;
    if !submit.status.success() {
        return Err(format!(
            "CLI submit process failed: {}",
            String::from_utf8_lossy(&submit.stderr)
        )
        .into());
    }

    // The node negotiates its allowed capabilities and must exit non-zero.
    let node = cargo_process(&root, "openoj-judge-node")
        .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
        .env("OPENOJ_JUDGE_NODE_ID", "judge_node_01")
        .env("OPENOJ_JUDGE_EXECUTOR", "development_mock")
        .output()?;
    if node.status.success() {
        return Err("judge node with unmatched capability must fail closed".into());
    }

    let status = cargo_process(&root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", &database_url)
        .arg("status")
        .arg(&evaluation_id)
        .output()?;
    if !status.status.success() {
        return Err("CLI status process failed".into());
    }
    let stdout = String::from_utf8(status.stdout)?;
    assert!(
        stdout.contains("state=queued"),
        "task must remain queued, got: {stdout}"
    );
    assert!(
        !stdout.contains("state=completed"),
        "task must not reach a terminal state, got: {stdout}"
    );

    control_plane.stop();
    Ok(())
}

/// A judge node whose identity is absent from the deployment allowlist must fail
/// closed during negotiation instead of claiming or executing anything.
#[test]
fn unknown_identity_judge_node_fails_closed() -> Result<(), Box<dyn Error>> {
    let test_db = FreshDatabase::new()?;
    let database_url = test_db.url().to_owned();
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let request_path = directory.path().join("request.json");
    let evaluation_id = unique_request(&request_path)?;
    let root = workspace_root()?;

    // The allowlist only authorizes `judge_node_99`; `judge_node_01` is unknown.
    let mut control_plane = ChildGuard::spawn(
        Command::new(env!("CARGO_BIN_EXE_openoj-control-plane"))
            .env("OPENOJ_DATABASE_URL", &database_url)
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
            .env("OPENOJ_JUDGE_NODES", "judge_node_99:algorithm.batch")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )?;
    wait_for_socket(&socket_path)?;

    let submit = cargo_process(&root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", &database_url)
        .arg("submit")
        .arg(&request_path)
        .output()?;
    if !submit.status.success() {
        return Err(format!(
            "CLI submit process failed: {}",
            String::from_utf8_lossy(&submit.stderr)
        )
        .into());
    }

    // The node reaches the server but its identity is denied -> exits non-zero.
    let node = cargo_process(&root, "openoj-judge-node")
        .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
        .env("OPENOJ_JUDGE_NODE_ID", "judge_node_01")
        .env("OPENOJ_JUDGE_EXECUTOR", "development_mock")
        .output()?;
    if node.status.success() {
        return Err("judge node with unknown identity must fail closed".into());
    }

    let status = cargo_process(&root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", &database_url)
        .arg("status")
        .arg(&evaluation_id)
        .output()?;
    let stdout = String::from_utf8(status.stdout)?;
    assert!(
        stdout.contains("state=queued"),
        "task must remain queued, got: {stdout}"
    );

    control_plane.stop();
    Ok(())
}

/// A judge node configured against a socket with no running control plane must
/// fail closed at connection instead of performing host-side execution.
#[test]
fn judge_node_without_a_control_listener_fails_closed() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let root = workspace_root()?;

    let node = cargo_process(&root, "openoj-judge-node")
        .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
        .env("OPENOJ_JUDGE_NODE_ID", "judge_node_01")
        .env("OPENOJ_JUDGE_EXECUTOR", "development_mock")
        .output()?;
    if node.status.success() {
        return Err("judge node without a control listener must fail closed".into());
    }
    Ok(())
}

/// An expired leased evaluation (the state a crashed judge node leaves behind) is recovered
/// by the control-plane sweeper into a new queued attempt and then completed by a fresh judge
/// node, proving end-to-end expired-lease recovery (ACC-P0-003 / FR-SCHED-001).
#[test]
fn expired_lease_is_recovered_and_completed_by_a_new_judge_node() -> Result<(), Box<dyn Error>> {
    let test_db = FreshDatabase::new()?;
    let database_url = test_db.url().to_owned();
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.path().join("judge-control.sock");
    let request_path = directory.path().join("request.json");
    let evaluation_id = unique_request(&request_path)?;
    let attempt_id = format!("attempt_{}", evaluation_id.trim_start_matches("eval_"));
    let root = workspace_root()?;

    let mut control_plane = ChildGuard::spawn(
        Command::new(env!("CARGO_BIN_EXE_openoj-control-plane"))
            .env("OPENOJ_DATABASE_URL", &database_url)
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
            .env("OPENOJ_JUDGE_NODES", "judge_node_01:algorithm.batch")
            .env("OPENOJ_LEASE_DURATION_MS", "1500")
            .env("OPENOJ_RENEW_AFTER_MS", "500")
            .env("OPENOJ_RECOVERY_INTERVAL_MS", "300")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )?;
    wait_for_socket(&socket_path)?;

    let submit = cargo_process(&root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", &database_url)
        .arg("submit")
        .arg(&request_path)
        .output()?;
    if !submit.status.success() {
        return Err(format!(
            "CLI submit process failed: {}",
            String::from_utf8_lossy(&submit.stderr)
        )
        .into());
    }

    // Simulate a judge node that claimed the task and then crashed: lease it in the past.
    let past = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())? - 1000;
    run_psql_url(
        &database_url,
        &format!(
            "UPDATE evaluations SET state='leased' WHERE evaluation_id='{evaluation_id}'; \
             UPDATE evaluation_attempts SET state='leased', node_id='judge_node_dead', \
                lease_token='lease_dead', lease_expires_at_ms={past} \
                WHERE evaluation_id='{evaluation_id}' AND attempt_number=1; \
             UPDATE evaluation_tasks SET state='leased' WHERE attempt_id='{attempt_id}';"
        ),
    )?;

    // The sweeper must recover the expired lease into a new queued attempt (attempt 2).
    wait_for_recovered_attempt(&root, &database_url, &evaluation_id)?;

    // A fresh judge node claims and completes the recovered attempt.
    let mut judge_node = ChildGuard::spawn(
        cargo_process(&root, "openoj-judge-node")
            .env("OPENOJ_JUDGE_CONTROL_SOCKET", &socket_path)
            .env("OPENOJ_JUDGE_NODE_ID", "judge_node_01")
            .env("OPENOJ_JUDGE_EXECUTOR", "development_mock")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )?;
    wait_for_terminal_status(&root, &database_url, &evaluation_id)?;
    judge_node.stop();
    control_plane.stop();
    Ok(())
}

/// A disposable per-test `PostgreSQL` database.
///
/// The full workspace test suite runs process tests and `#[sqlx::test]` storage
/// tests in parallel, so sharing one database would race migrations and submits.
/// Each process test creates and drops its own isolated database.
struct FreshDatabase {
    url: String,
    name: String,
}

impl FreshDatabase {
    fn new() -> Result<Self, Box<dyn Error>> {
        // Two tests in one process can share the same clock tick, so a per-process
        // counter keeps the database name unique. Drop any crashed-run leftover of
        // the same name before creating.
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let base = env::var("OPENOJ_TEST_DATABASE_URL")?;
        let (head, _) = base.rsplit_once('/').ok_or("invalid test database url")?;
        let name = format!(
            "openoj_p0c_{}_{}_{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        run_psql(
            head,
            &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        )?;
        run_psql(head, &format!("CREATE DATABASE {name}"))?;
        Ok(Self {
            url: format!("{head}/{name}"),
            name,
        })
    }

    fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for FreshDatabase {
    fn drop(&mut self) {
        let head = self
            .url
            .rsplit_once('/')
            .map(|(head, _)| head.to_owned())
            .unwrap_or_default();
        let _ignored = run_psql(
            &head,
            &format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        );
    }
}

fn run_psql(connection_head: &str, sql: &str) -> Result<(), Box<dyn Error>> {
    let rest = connection_head.trim_start_matches("postgres://");
    let (userinfo, hostport) = rest.split_once('@').ok_or("invalid connection url")?;
    let (user, password) = userinfo.split_once(':').ok_or("invalid connection url")?;
    let (host, port) = hostport.rsplit_once(':').ok_or("invalid connection url")?;
    let output = Command::new("psql")
        .env("PGPASSWORD", password)
        .args([
            "-h", host, "-p", port, "-U", user, "-d", "postgres", "-c", sql,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("psql failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(())
}

fn run_psql_url(url: &str, sql: &str) -> Result<(), Box<dyn Error>> {
    let rest = url.trim_start_matches("postgres://");
    let (userinfo, hostport_database) = rest.split_once('@').ok_or("invalid connection url")?;
    let (user, password) = userinfo.split_once(':').ok_or("invalid connection url")?;
    let (hostport, database) = hostport_database
        .split_once('/')
        .ok_or("invalid connection url")?;
    let (host, port) = hostport.rsplit_once(':').ok_or("invalid connection url")?;
    let output = Command::new("psql")
        .env("PGPASSWORD", password)
        .args([
            "-h", host, "-p", port, "-U", user, "-d", database, "-c", sql,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("psql failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(())
}

fn persisted_result(
    url: &str,
    evaluation_id: &str,
) -> Result<openoj_domain::EvaluationResult, Box<dyn Error>> {
    let rest = url.trim_start_matches("postgres://");
    let (userinfo, hostport_database) = rest.split_once('@').ok_or("invalid connection url")?;
    let (user, password) = userinfo.split_once(':').ok_or("invalid connection url")?;
    let (hostport, database) = hostport_database
        .split_once('/')
        .ok_or("invalid connection url")?;
    let (host, port) = hostport.rsplit_once(':').ok_or("invalid connection url")?;
    let sql = format!(
        "SELECT convert_from(terminal_result, 'UTF8') FROM evaluations \
         WHERE evaluation_id = '{evaluation_id}'"
    );
    let output = Command::new("psql")
        .env("PGPASSWORD", password)
        .args([
            "-h", host, "-p", port, "-U", user, "-d", database, "-At", "-c", &sql,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("psql failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(decode_evaluation_result_domain(
        String::from_utf8(output.stdout)?.trim().as_bytes(),
    )?)
}

fn unique_request(path: &Path) -> Result<String, Box<dyn Error>> {
    let milliseconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let label = format!("p{}_{}", std::process::id(), milliseconds);
    let request = VALID_REQUEST
        .replace("req_01", &format!("req_{label}"))
        .replace("idem_01", &format!("idem_{label}"))
        .replace("eval_01", &format!("eval_{label}"))
        .replace("attempt_01", &format!("attempt_{label}"));
    fs::write(path, request)?;
    Ok(format!("eval_{label}"))
}

fn unique_firecracker_request(
    path: &Path,
    runtime_digest: &str,
    source_sha256: &str,
    source_size: usize,
    wall_time_ms: u64,
) -> Result<String, Box<dyn Error>> {
    let milliseconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let label = format!("fc{}_{}", std::process::id(), milliseconds);
    let runtime_hex = runtime_digest
        .strip_prefix("sha256:")
        .ok_or("invalid runtime digest")?;
    let request = VALID_REQUEST
        .replace("req_01", &format!("req_{label}"))
        .replace("idem_01", &format!("idem_{label}"))
        .replace("eval_01", &format!("eval_{label}"))
        .replace("attempt_01", &format!("attempt_{label}"))
        .replace(&"2".repeat(64), source_sha256)
        .replace("text/x-c++src", "text/x-csrc")
        .replace(
            "\"size_bytes\": 128",
            &format!("\"size_bytes\": {source_size}"),
        )
        .replace(
            "\"wall_time_ms\": 2000",
            &format!("\"wall_time_ms\": {wall_time_ms}"),
        )
        .replace("runtime_cpp_01", "runtime_algorithm_c_v0alpha1")
        .replace(&"3".repeat(64), runtime_hex);
    fs::write(path, request)?;
    Ok(format!("eval_{label}"))
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .ok_or_else(|| "workspace root unavailable".into())
}

fn cargo_process(root: &Path, package: &str) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(["run", "--quiet", "--locked", "-p", package, "--"]);
    command
}

fn wait_for_socket(path: &Path) -> Result<(), Box<dyn Error>> {
    wait_for_path(path, "control-plane UDS listener")
}

fn wait_for_path(path: &Path, description: &str) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("timed out waiting for {description}").into())
}

fn current_unix_millis() -> Result<UnixMillis, Box<dyn Error>> {
    let milliseconds = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    Ok(UnixMillis::new(milliseconds)?)
}

fn assert_cancelled_status(
    root: &Path,
    database_url: &str,
    evaluation_id: &str,
) -> Result<(), Box<dyn Error>> {
    let status = cargo_process(root, "openoj-cli")
        .env("OPENOJ_DATABASE_URL", database_url)
        .arg("status")
        .arg(evaluation_id)
        .output()?;
    if !status.status.success() {
        return Err(format!(
            "CLI status process failed: {}",
            String::from_utf8_lossy(&status.stderr)
        )
        .into());
    }
    let stdout = String::from_utf8(status.stdout)?;
    assert!(
        stdout.contains("state=cancelled terminal_result=true"),
        "cancelled terminal result must remain visible, got: {stdout}"
    );
    Ok(())
}

fn wait_for_terminal_status(
    root: &Path,
    database_url: &str,
    evaluation_id: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let status = cargo_process(root, "openoj-cli")
            .env("OPENOJ_DATABASE_URL", database_url)
            .arg("status")
            .arg(evaluation_id)
            .output()?;
        if status.status.success()
            && String::from_utf8(status.stdout)?.contains("state=completed terminal_result=true")
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("judge-node did not produce a terminal result".into())
}

fn wait_for_any_terminal_status(
    root: &Path,
    database_url: &str,
    evaluation_id: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let status = cargo_process(root, "openoj-cli")
            .env("OPENOJ_DATABASE_URL", database_url)
            .arg("status")
            .arg(evaluation_id)
            .output()?;
        if status.status.success()
            && String::from_utf8(status.stdout)?.contains("terminal_result=true")
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("judge-node did not persist a terminal result".into())
}

fn wait_for_recovered_attempt(
    root: &Path,
    database_url: &str,
    evaluation_id: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let status = cargo_process(root, "openoj-cli")
            .env("OPENOJ_DATABASE_URL", database_url)
            .arg("status")
            .arg(evaluation_id)
            .output()?;
        let stdout = String::from_utf8(status.stdout)?;
        if status.status.success()
            && stdout.contains("attempt_number=2")
            && stdout.contains("state=queued")
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err("control-plane sweeper did not recover the expired lease into attempt 2".into())
}

struct ChildGuard(Child);

impl ChildGuard {
    fn spawn(command: &mut Command) -> Result<Self, Box<dyn Error>> {
        Ok(Self(command.spawn()?))
    }

    fn stop(&mut self) {
        let _ignored = self.0.kill();
        let _ignored = self.0.wait();
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus, Box<dyn Error>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err("child process did not exit before the deadline".into())
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
    }
}
