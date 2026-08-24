//! Real-KVM algorithm-c integration smoke for the Firecracker adapter.
//!
//! Normal CI skips when KVM, Firecracker, or provisioned images are absent.
//! `OPENOJ_REQUIRE_KVM=1` turns every skip condition into a test failure. The
//! test verifies the pinned image manifest, boots fresh guests, negotiates the
//! bounded protocol, uploads digest-checked C source, exercises success and
//! failure stages, and verifies idempotent resource reclamation.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;

use openoj_firecracker::{
    FirecrackerConfig, FirecrackerConfigParts, FirecrackerVm, GuestChannel, MachineConfig,
    ResourceLimits, VmPhase, VsockConfig,
};
use openoj_guest_protocol::{Message, Stage};
use serde_json::Value;

type TestResult<T> = Result<T, Box<dyn Error>>;

const SUCCESS_SOURCE: &[u8] = b"#include <stdio.h>\nint main(void) { puts(\"42\"); return 0; }\n";
const SUCCESS_SOURCE_SHA256: &str =
    "c2329bffd207edd619d4ca7c7cdd872b76374e1a50421f705020671ca1f85556";
const SUCCESS_OUTPUT_SHA256: &str =
    "084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0";
const COMPILE_ERROR_SOURCE: &[u8] = b"int main(void) { this is not C; }\n";
const COMPILE_ERROR_SOURCE_SHA256: &str =
    "45c508a05870369dd65fb951ee804127925b454fa0078ac6e37f84f5a1e3fa06";
const TIMEOUT_SOURCE: &[u8] = b"int main(void) { for (;;) {} }\n";
const TIMEOUT_SOURCE_SHA256: &str =
    "84950edbf9514ebef845181f9f296bf2a71be3032f0d1d27b28b93fe1fd57f54";
const TIMEOUT_EXIT_CODE: i32 = 124;

struct RuntimeImages {
    kernel: PathBuf,
    kernel_sha256: String,
    rootfs: PathBuf,
    rootfs_sha256: String,
}

#[derive(Clone, Copy)]
enum SmokeCase {
    Success,
    CompileError,
    RunTimeout,
}

impl SmokeCase {
    const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::CompileError => "compile-error",
            Self::RunTimeout => "run-timeout",
        }
    }

    const fn source(self) -> (&'static [u8], &'static str) {
        match self {
            Self::Success => (SUCCESS_SOURCE, SUCCESS_SOURCE_SHA256),
            Self::CompileError => (COMPILE_ERROR_SOURCE, COMPILE_ERROR_SOURCE_SHA256),
            Self::RunTimeout => (TIMEOUT_SOURCE, TIMEOUT_SOURCE_SHA256),
        }
    }
}

fn strict_mode() -> bool {
    env::var("OPENOJ_REQUIRE_KVM").as_deref() == Ok("1")
}

fn unavailable(reason: &str) -> TestResult<()> {
    if strict_mode() {
        return Err(format!("strict KVM smoke requirement not met: {reason}").into());
    }
    println!("skipping real-KVM algorithm-c smoke: {reason}");
    Ok(())
}

fn firecracker_path() -> PathBuf {
    env::var_os("OPENOJ_FC_FIRECRACKER").map_or_else(|| PathBuf::from("firecracker"), PathBuf::from)
}

fn kvm_accessible() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok()
}

fn firecracker_available(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn image_root() -> PathBuf {
    env::var_os("OPENOJ_FC_TEST_IMAGES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../infra/runtime-images/algorithm-c/out"),
        PathBuf::from,
    )
}

fn manifest_string<'a>(manifest: &'a Value, path: &[&str]) -> TestResult<&'a str> {
    let mut value = manifest;
    for component in path {
        value = value
            .get(component)
            .ok_or_else(|| format!("manifest field is missing: {}", path.join(".")))?;
    }
    value
        .as_str()
        .ok_or_else(|| format!("manifest field is not a string: {}", path.join(".")).into())
}

fn valid_raw_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn load_runtime_images() -> TestResult<Option<RuntimeImages>> {
    let root = image_root();
    let manifest_path = root.join("manifest.json");
    let checksum_path = root.join("manifest.sha256");
    let kernel = root.join("kernel/vmlinux.bin");
    let rootfs = root.join("rootfs/rootfs.ext4");
    if !manifest_path.is_file()
        || !checksum_path.is_file()
        || !kernel.is_file()
        || !rootfs.is_file()
    {
        return Ok(None);
    }

    let checksum_output = Command::new("sha256sum")
        .arg("--check")
        .arg("--strict")
        .arg("manifest.sha256")
        .current_dir(&root)
        .output()?;
    if !checksum_output.status.success() {
        return Err(format!(
            "runtime image checksum verification failed: {}",
            String::from_utf8_lossy(&checksum_output.stderr)
        )
        .into());
    }

    let checksums = std::fs::read_to_string(&checksum_path)?
        .lines()
        .map(|line| {
            line.split_once("  ")
                .map(|(digest, name)| (name.to_owned(), digest.to_owned()))
                .ok_or_else(|| format!("malformed checksum line: {line}"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    if manifest.get("runtime").and_then(Value::as_str) != Some("algorithm-c")
        || manifest.get("version").and_then(Value::as_str) != Some("v0alpha1")
        || manifest.get("development_only").and_then(Value::as_bool) != Some(true)
        || manifest.get("production_eligible").and_then(Value::as_bool) != Some(false)
        || manifest.get("rootfs_read_only").and_then(Value::as_bool) != Some(true)
        || manifest.get("guest_network").and_then(Value::as_str) != Some("absent")
    {
        return Err("runtime manifest violates the algorithm-c development contract".into());
    }

    let kernel_sha256 = manifest_string(&manifest, &["images", "kernel", "output_sha256"])?;
    let rootfs_sha256 = manifest_string(&manifest, &["images", "rootfs", "output_sha256"])?;
    if !valid_raw_sha256(kernel_sha256) || !valid_raw_sha256(rootfs_sha256) {
        return Err("runtime manifest contains a malformed image digest".into());
    }
    if checksums.get("kernel/vmlinux.bin").map(String::as_str) != Some(kernel_sha256)
        || checksums.get("rootfs/rootfs.ext4").map(String::as_str) != Some(rootfs_sha256)
    {
        return Err("runtime manifest digest does not match manifest.sha256".into());
    }

    Ok(Some(RuntimeImages {
        kernel: std::fs::canonicalize(kernel)?,
        kernel_sha256: kernel_sha256.to_owned(),
        rootfs: std::fs::canonicalize(rootfs)?,
        rootfs_sha256: rootfs_sha256.to_owned(),
    }))
}

fn vm_config(images: &RuntimeImages, vsock_uds_path: PathBuf) -> TestResult<FirecrackerConfig> {
    Ok(FirecrackerConfig::from_parts(FirecrackerConfigParts {
        kernel_path: images.kernel.clone(),
        kernel_digest: format!("sha256:{}", images.kernel_sha256),
        rootfs_path: images.rootfs.clone(),
        rootfs_digest: format!("sha256:{}", images.rootfs_sha256),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off root=/dev/vda ro init=/sbin/openoj-init"
            .to_owned(),
        machine: MachineConfig::new(1, 256)?,
        limits: ResourceLimits::default(),
        vsock: VsockConfig::new(3, 8266)?,
        vsock_uds_path,
        jailer_path: None,
    })?)
}

fn exchange(channel: &mut GuestChannel, message: &Message) -> TestResult<Message> {
    channel.send(message)?;
    Ok(channel.recv()?)
}

fn build_message() -> Message {
    Message::Build {
        argv: vec![
            "/usr/bin/cc".to_owned(),
            "-std=c17".to_owned(),
            "-O2".to_owned(),
            "-pipe".to_owned(),
            "/work/inputs/main.c".to_owned(),
            "-o".to_owned(),
            "/work/solution".to_owned(),
        ],
        wall_time_ms: 30_000,
    }
}

fn exercise_guest(vm: &mut FirecrackerVm, case: SmokeCase) -> TestResult<()> {
    let mut channel = vm.open_guest_channel()?;
    let negotiated = exchange(
        &mut channel,
        &Message::Negotiate {
            capabilities: vec!["algorithm.batch".to_owned()],
        },
    )?;
    if negotiated != (Message::Negotiated { supported: true }) {
        return Err("guest rejected algorithm.batch negotiation".into());
    }

    let (source, digest) = case.source();
    let upload = exchange(
        &mut channel,
        &Message::UploadInput {
            name: "main.c".to_owned(),
            digest: digest.to_owned(),
            bytes: source.to_vec(),
        },
    )?;
    if upload
        != (Message::UploadAck {
            name: "main.c".to_owned(),
            accepted: true,
        })
    {
        return Err("guest rejected the digest-checked C fixture".into());
    }

    let build = exchange(&mut channel, &build_message())?;
    let Message::StageOutput {
        stage,
        exit_code,
        diagnostics,
        ..
    } = build
    else {
        return Err("guest returned a non-stage response for build".into());
    };
    if stage != Stage::Build {
        return Err("guest returned the wrong build stage".into());
    }
    if matches!(case, SmokeCase::CompileError) {
        if exit_code == 0 || diagnostics.is_empty() {
            return Err("invalid C fixture did not produce bounded compile diagnostics".into());
        }
        return Ok(());
    }
    if exit_code != 0 {
        return Err(format!("valid C fixture failed to compile with exit code {exit_code}").into());
    }

    let run = exchange(
        &mut channel,
        &Message::Run {
            argv: vec!["/work/solution".to_owned()],
            wall_time_ms: if matches!(case, SmokeCase::RunTimeout) {
                50
            } else {
                2_000
            },
        },
    )?;
    let Message::StageOutput {
        stage,
        exit_code,
        output_digest,
        output_bytes,
        diagnostics,
        ..
    } = run
    else {
        return Err("guest returned a non-stage response for run".into());
    };
    if stage != Stage::Run {
        return Err("guest returned the wrong run stage".into());
    }
    match case {
        SmokeCase::Success => {
            if exit_code != 0 || output_digest != SUCCESS_OUTPUT_SHA256 || output_bytes != 3 {
                return Err("successful C fixture output evidence did not match".into());
            }
        }
        SmokeCase::RunTimeout => {
            if exit_code != TIMEOUT_EXIT_CODE || diagnostics.is_empty() {
                return Err("infinite C fixture did not produce a bounded timeout".into());
            }
        }
        SmokeCase::CompileError => {
            return Err("compile-error fixture unexpectedly reached run".into());
        }
    }
    Ok(())
}

async fn run_case(images: &RuntimeImages, firecracker: &Path, case: SmokeCase) -> TestResult<()> {
    let work = env::temp_dir().join(format!(
        "openoj-algorithm-c-{}-{}",
        case.name(),
        std::process::id()
    ));
    let _ = tokio::fs::remove_dir_all(&work).await;
    tokio::fs::create_dir_all(&work).await?;
    let api_socket = work.join("firecracker.sock");
    let vsock_socket = work.join("vsock.sock");
    let config = vm_config(images, vsock_socket.clone())?;
    let mut vm = FirecrackerVm::new(config, firecracker, &api_socket);

    let outcome = async {
        vm.bootstrap(false).await?;
        exercise_guest(&mut vm, case)
    }
    .await;
    let first_termination = vm.terminate().await;
    let phase = vm.phase();
    let second_termination = vm.terminate().await;
    let api_socket_reclaimed = !api_socket.exists();
    let vsock_socket_reclaimed = !vsock_socket.exists();
    let cleanup = tokio::fs::remove_dir_all(&work).await;

    outcome?;
    first_termination?;
    second_termination?;
    cleanup?;
    if phase != VmPhase::Terminated || !api_socket_reclaimed || !vsock_socket_reclaimed {
        return Err("Firecracker teardown left lifecycle or socket resources behind".into());
    }
    Ok(())
}

#[tokio::test]
async fn algorithm_c_guest_compiles_runs_and_reclaims() -> TestResult<()> {
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

    for case in [
        SmokeCase::Success,
        SmokeCase::CompileError,
        SmokeCase::RunTimeout,
    ] {
        run_case(&images, &firecracker, case).await?;
    }
    Ok(())
}
