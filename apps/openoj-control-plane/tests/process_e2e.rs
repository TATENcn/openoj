use std::env;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const VALID_REQUEST: &str =
    include_str!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

#[test]
fn postgres_uds_and_two_child_processes_reach_a_terminal_mock_result() -> Result<(), Box<dyn Error>>
{
    let database_url = env::var("OPENOJ_TEST_DATABASE_URL")?;
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
