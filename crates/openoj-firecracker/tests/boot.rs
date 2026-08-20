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

use openoj_firecracker::{
    FirecrackerConfig, FirecrackerConfigParts, FirecrackerError, MachineConfig, ResourceLimits,
    VmPhase, VsockConfig,
};

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
            .map(|output| output.status.success())
            .unwrap_or(false)
}

fn config() -> Result<Option<FirecrackerConfig>, FirecrackerError> {
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
    let Some(config) = config()? else {
        println!("skipping: runtime images not provisioned");
        return Ok(());
    };

    let work = env::temp_dir().join(format!("openoj-fc-boot-{}", std::process::id()));
    tokio::fs::create_dir_all(&work).await?;
    let api_socket = work.join("fc.sock");
    let mut vm = openoj_firecracker::FirecrackerVm::new(
        config,
        "/usr/bin/firecracker",
        &api_socket,
    );
    vm.bootstrap(false).await?;
    assert!(vm.can_communicate(), "microVM did not reach started phase");

    vm.terminate().await?;
    assert_eq!(vm.phase(), VmPhase::Terminated);
    let _ = tokio::fs::remove_dir_all(&work).await;
    Ok(())
}
