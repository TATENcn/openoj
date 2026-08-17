use std::env;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const VALID_REQUEST: &str =
    include_str!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

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
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("control-plane did not create its UDS listener".into())
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
    Err("judge-node did not produce a terminal development-mock result".into())
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
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
    }
}
