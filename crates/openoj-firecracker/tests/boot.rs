//! Real-KVM boot integration test for the `openoj-firecracker` adapter.
//!
//! This test is gated on a host `/dev/kvm` device, the `firecracker` binary, and
//! the presence of pre-provisioned runtime images. CI runners without KVM skip
//! it; on hosts with KVM it proves that `FirecrackerVm` can launch, configure,
//! start, and reclaim a real microVM. It is **not** production isolation or
//! performance evidence.
//!
//! Images are read from `$OPENOJ_FC_TEST_IMAGES` (default `infra/runtime-images/
//! algorithm-c/out`). `infra/runtime-images/algorithm-c/provision.sh` produces
//! this layout from pinned sources.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use openoj_firecracker::{
    FirecrackerConfig, FirecrackerConfigParts, FirecrackerError, GuestChannel, MachineConfig,
    ResourceLimits, VmPhase, VsockConfig,
};
use openoj_guest_protocol::Message;

fn image_root() -> Option<PathBuf> {
    env::var_os("OPENOJ_FC_TEST_IMAGES")
        .map(PathBuf::from)
        .or_else(|| Some(Path::new("infra/runtime-images/algorithm-c/out").to_path_buf()))
}

fn kvm_available() -> bool {
    Path::new("/dev/kvm").exists()
        && Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
}

fn config(vsock_uds_path: &Path) -> Result<Option<FirecrackerConfig>, FirecrackerError> {
    let Some(root) = image_root() else {
        return Ok(None);
    };
    let kernel = root.join("kernel/vmlinux.bin");
    let rootfs = root.join("rootfs/rootfs.ext4");
    if !kernel.exists() || !rootfs.exists() {
        return Ok(None);
    }
    FirecrackerConfig::from_parts(FirecrackerConfigParts {
        kernel_path: kernel,
        kernel_digest: "sha256:unverified-dev".to_owned(),
        rootfs_path: rootfs,
        rootfs_digest: "sha256:unverified-dev".to_owned(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_owned(),
        machine: MachineConfig::new(1, 128)?,
        limits: ResourceLimits::default(),
        vsock: VsockConfig::new(3, 8266)?,
        vsock_uds_path: vsock_uds_path.to_path_buf(),
        jailer_path: None,
    })
    .map(Some)
}

#[tokio::test]
async fn boots_a_real_microvm_and_reclaims_it() -> Result<(), Box<dyn std::error::Error>> {
    if !kvm_available() {
        println!("skipping: /dev/kvm or firecracker not available");
        return Ok(());
    }
    let work = env::temp_dir().join(format!("openoj-fc-boot-{}", std::process::id()));
    tokio::fs::create_dir_all(&work).await?;
    let vsock_uds = work.join("fc-vsock.sock");
    let Some(config) = config(&vsock_uds)? else {
        println!("skipping: runtime images not provisioned");
        return Ok(());
    };

    let api_socket = work.join("fc.sock");
    let mut vm =
        openoj_firecracker::FirecrackerVm::new(config, "/usr/bin/firecracker", &api_socket);
    vm.bootstrap(false).await?;
    assert!(vm.can_communicate(), "microVM did not reach started phase");

    vm.terminate().await?;
    assert_eq!(vm.phase(), VmPhase::Terminated);
    let _ = tokio::fs::remove_dir_all(&work).await;
    Ok(())
}

/// Real-KVM round-trip: host drives the in-guest agent across the vsock bridge.
///
/// Gated exactly like `boots_a_real_microvm_and_reclaims_it`. Proves the full
/// execution-plane data path on real hardware: guest boots to the agent, host
/// connects over the bridged vsock, negotiates capabilities, uploads an input,
/// runs a validated build-stage command, reads the bounded stage output, and
/// tears the microVM down. Not production isolation or performance evidence.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn guest_agent_round_trip_over_vsock() -> Result<(), Box<dyn std::error::Error>> {
    if !kvm_available() {
        println!("skipping: /dev/kvm or firecracker not available");
        return Ok(());
    }
    let Some(root) = image_root() else {
        return Ok(());
    };
    let kernel = root.join("kernel/vmlinux.bin");
    let rootfs = root.join("rootfs/rootfs.ext4");
    if !kernel.exists() || !rootfs.exists() {
        println!("skipping: runtime images not provisioned (no vsock round-trip)");
        return Ok(());
    }

    let work = env::temp_dir().join(format!("openoj-fc-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let api_socket = work.join("fc.sock");
    // Absolute UDS path; Firecracker bridges the guest vsock here.
    let vsock_uds = work.join("fc-vsock.sock");
    let port = 8266u32;

    let config = FirecrackerConfig::from_parts(FirecrackerConfigParts {
        kernel_path: kernel,
        kernel_digest: "sha256:unverified-dev".to_owned(),
        rootfs_path: rootfs,
        rootfs_digest: "sha256:unverified-dev".to_owned(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off init=/bin/openoj-init".to_owned(),
        machine: MachineConfig::new(1, 128)?,
        limits: ResourceLimits::default(),
        vsock: VsockConfig::new(3, port)?,
        vsock_uds_path: vsock_uds.clone(),
        jailer_path: None,
    })?;

    let mut vm =
        openoj_firecracker::FirecrackerVm::new(config, "/usr/bin/firecracker", &api_socket);
    vm.bootstrap(false).await?;
    assert!(vm.can_communicate(), "microVM did not reach started phase");

    // Let the guest finish booting to the agent before opening the vsock
    // channel: a CONNECT before the agent binds can be accepted optimistically
    // by Firecracker's vsock backend and then dropped, which would terminate
    // the single-session in-guest agent. Measured boot-to-bind is <1s on KVM.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Wait for the guest to boot to the agent and bind the vsock port. A
    // CONNECT before the agent binds is refused at the device level and
    // consumes nothing, so retrying the connect alone is safe: once the
    // handshake returns OK the guest listener is accepting, and the exchange
    // below is committed to that connection (never dropped mid-way, which
    // would terminate the single-session agent).
    let deadline = Instant::now() + Duration::from_mins(1);
    let mut channel = loop {
        match GuestChannel::connect(&vsock_uds, port, Duration::from_secs(15)) {
            Ok(channel) => break channel,
            Err(error) if Instant::now() < deadline => {
                println!("vsock connect not ready: {error}; retrying");
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            Err(error) => {
                let _ = vm.terminate().await;
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("guest agent never accepted vsock: {error}"),
                )) as Box<dyn std::error::Error>);
            }
        }
    };

    // Negotiate capabilities.
    channel.send(&Message::Negotiate {
        capabilities: vec!["algorithm.batch".to_owned()],
    })?;
    let negotiated = channel.recv()?;
    assert_eq!(
        negotiated,
        Message::Negotiated { supported: true },
        "guest agent must accept supported capabilities"
    );

    // Upload an input artifact.
    channel.send(&Message::UploadInput {
        name: "greeting.txt".to_owned(),
        digest: "sha256:unverified".to_owned(),
        bytes: b"hello from openoj\n".to_vec(),
    })?;
    let ack = channel.recv()?;
    assert_eq!(
        ack,
        Message::UploadAck {
            name: "greeting.txt".to_owned(),
            accepted: true,
        }
    );

    // Run a validated build-stage command in the guest.
    channel.send(&Message::Build {
        argv: vec!["/bin/echo".to_owned(), "hello openoj".to_owned()],
        wall_time_ms: 5_000,
    })?;
    let (exit_code, output_digest) = match channel.recv()? {
        Message::StageOutput {
            exit_code,
            output_digest,
            ..
        } => (exit_code, output_digest),
        other => {
            let _ = vm.terminate().await;
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unexpected reply to build stage: {other:?}"),
            )) as Box<dyn std::error::Error>);
        }
    };
    assert_eq!(exit_code, 0, "guest build stage must succeed");
    assert!(
        !output_digest.is_empty(),
        "stage output must carry a digest"
    );

    // Heartbeat confirms the agent is still alive.
    channel.send(&Message::Heartbeat)?;
    assert_eq!(channel.recv()?, Message::Ack);

    vm.terminate().await?;
    assert_eq!(vm.phase(), VmPhase::Terminated);
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}
