//! Lifecycle driving of a Firecracker microVM process.
//!
//! [`FirecrackerVm`] owns one firecracker/jailer process for one task, drives
//! its control API over a Unix socket, opens the guest vsock channel, and
//! guarantees an idempotent teardown. It is the only crate responsible for
//! process and privileged resource ownership.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::time::sleep;

use crate::api::{self, ApiResponse};
use crate::config::{FirecrackerConfig, FirecrackerError};
use crate::lifecycle::{Lifecycle, TeardownReason, VmPhase};
use crate::vsock::GuestChannel;

/// Time to wait for the Firecracker API socket file to appear after spawn.
pub const SOCKET_WAIT: Duration = Duration::from_secs(10);

/// Poll interval while waiting for the API socket.
pub const SOCKET_POLL: Duration = Duration::from_millis(50);

/// Response type for successful API calls.
const CONTENT_TYPE_JSON: &str = "application/json";

/// Drives a single Firecracker microVM for one task.
pub struct FirecrackerVm {
    config: FirecrackerConfig,
    lifecycle: Lifecycle,
    firecracker_path: PathBuf,
    api_socket: PathBuf,
    child: Option<Child>,
}

impl FirecrackerVm {
    /// Builds a VM handle without launching a process.
    #[must_use]
    pub fn new(
        config: FirecrackerConfig,
        firecracker_path: impl AsRef<Path>,
        api_socket: impl AsRef<Path>,
    ) -> Self {
        Self {
            config,
            lifecycle: Lifecycle::new(),
            firecracker_path: firecracker_path.as_ref().to_path_buf(),
            api_socket: api_socket.as_ref().to_path_buf(),
            child: None,
        }
    }

    /// Current lifecycle phase.
    #[must_use]
    pub const fn phase(&self) -> VmPhase {
        self.lifecycle.phase()
    }

    /// Whether the VM has booted and a guest channel may be opened.
    #[must_use]
    pub const fn can_communicate(&self) -> bool {
        self.lifecycle.phase().can_communicate()
    }

    /// Launches, configures, and starts the microVM.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError`] when the process cannot spawn, the API
    /// socket does not appear, a control request fails, or `production` is set
    /// without a configured jailer.
    pub async fn bootstrap(&mut self, production: bool) -> Result<(), FirecrackerError> {
        self.lifecycle.launch().map_err(|_| FirecrackerError::Io {
            message: "lifecycle cannot launch".to_owned(),
        })?;
        if production && self.config.jailer_path().is_none() {
            self.lifecycle.fail_with(TeardownReason::ControlApi);
            return Err(FirecrackerError::ProductionRequiresJailer);
        }

        self.spawn_process()?;
        self.wait_for_socket().await?;
        self.lifecycle
            .configuring()
            .map_err(|_| FirecrackerError::Io {
                message: "lifecycle cannot configure".to_owned(),
            })?;
        self.configure().await?;
        self.start().await?;
        self.lifecycle.start().map_err(|_| FirecrackerError::Io {
            message: "lifecycle cannot start".to_owned(),
        })?;
        Ok(())
    }

    fn spawn_process(&mut self) -> Result<(), FirecrackerError> {
        let jailer = self.config.jailer_path().map(Path::to_path_buf);
        let child = match jailer {
            Some(jailer_path) => self.spawn_jailer(&jailer_path)?,
            None => self.spawn_direct()?,
        };
        self.child = Some(child);
        Ok(())
    }

    fn spawn_direct(&self) -> Result<Child, FirecrackerError> {
        Command::new(&self.firecracker_path)
            .arg("--api-sock")
            .arg(&self.api_socket)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| FirecrackerError::Io {
                message: format!("spawn firecracker failed: {error}"),
            })
    }

    fn spawn_jailer(&self, jailer_path: &Path) -> Result<Child, FirecrackerError> {
        // The jailer chroots into <chroot-base>/<uid>/<gid>/<id> and re-execs
        // the firecracker binary. The API socket is placed inside that root;
        // parents must exist and be owned by the configured identity.
        let chroot_base = self
            .api_socket
            .parent()
            .ok_or_else(|| FirecrackerError::Io {
                message: "api socket has no parent".to_owned(),
            })?;
        let socket_name = self
            .api_socket
            .file_name()
            .ok_or_else(|| FirecrackerError::Io {
                message: "api socket has no file name".to_owned(),
            })?;
        let jailer_id = "openoj";
        Command::new(jailer_path)
            .arg("--id")
            .arg(jailer_id)
            .arg("--exec-file")
            .arg(&self.firecracker_path)
            .arg("--chroot-base-dir")
            .arg(chroot_base)
            .arg("--")
            .arg("--api-sock")
            .arg(socket_name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| FirecrackerError::Io {
                message: format!("spawn jailer failed: {error}"),
            })
    }

    async fn wait_for_socket(&mut self) -> Result<(), FirecrackerError> {
        let deadline = std::time::Instant::now() + SOCKET_WAIT;
        loop {
            if tokio::fs::try_exists(&self.api_socket)
                .await
                .unwrap_or(false)
            {
                return Ok(());
            }
            if self.child_finished() {
                self.lifecycle.fail_with(TeardownReason::UnexpectedExit);
                return Err(FirecrackerError::Io {
                    message: "firecracker exited before opening the API socket".to_owned(),
                });
            }
            if std::time::Instant::now() >= deadline {
                self.lifecycle.fail_with(TeardownReason::UnexpectedExit);
                return Err(FirecrackerError::Io {
                    message: "timed out waiting for the API socket".to_owned(),
                });
            }
            sleep(SOCKET_POLL).await;
        }
    }

    fn child_finished(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => child.try_wait().ok().flatten().is_some(),
            None => true,
        }
    }

    async fn configure(&mut self) -> Result<(), FirecrackerError> {
        self.put_boot_source().await?;
        self.put_root_drive().await?;
        self.put_machine_config().await?;
        self.put_vsock().await
    }

    async fn api<T: serde::Serialize>(
        &mut self,
        method: &str,
        target: &str,
        value: &T,
    ) -> Result<(), FirecrackerError> {
        let body = serde_json::to_vec(value).map_err(|error| FirecrackerError::Io {
            message: format!("serialize API body failed: {error}"),
        })?;
        let _: ApiResponse = api::request(
            &self.api_socket,
            method,
            target,
            CONTENT_TYPE_JSON,
            Some(&body),
        )
        .await
        .inspect_err(|_| self.lifecycle.fail_with(TeardownReason::ControlApi))?;
        Ok(())
    }

    async fn put_boot_source(&mut self) -> Result<(), FirecrackerError> {
        self.api(
            "PUT",
            "/boot-source",
            &serde_json::json!({
                "kernel_image_path": self.config.kernel_path(),
                "boot_args": self.config.boot_args()
            }),
        )
        .await
    }

    async fn put_root_drive(&mut self) -> Result<(), FirecrackerError> {
        self.api(
            "PUT",
            "/drives/rootfs",
            &serde_json::json!({
                "drive_id": "rootfs",
                "path_on_host": self.config.rootfs_path(),
                "is_root_device": true,
                "is_read_only": true
            }),
        )
        .await
    }

    async fn put_machine_config(&mut self) -> Result<(), FirecrackerError> {
        self.api(
            "PUT",
            "/machine-config",
            &serde_json::json!({
                "vcpu_count": self.config.machine().vcpu_count(),
                "mem_size_mib": self.config.machine().mem_size_mib(),
                "smt": false,
                "track_dirty_pages": false
            }),
        )
        .await
    }

    async fn put_vsock(&mut self) -> Result<(), FirecrackerError> {
        self.api(
            "PUT",
            "/vsock",
            &serde_json::json!({
                "vsock_id": "vsock0",
                "guest_cid": self.config.vsock().guest_cid(),
                "uds_path": self.config.vsock_uds_path()
            }),
        )
        .await
    }

    async fn start(&mut self) -> Result<(), FirecrackerError> {
        self.api(
            "PUT",
            "/actions",
            &serde_json::json!({ "action_type": "InstanceStart" }),
        )
        .await
    }

    /// Opens the host→guest vsock channel after the guest agent is listening.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError`] when not yet started or the connection fails.
    pub fn open_guest_channel(&self) -> Result<GuestChannel, FirecrackerError> {
        if !self.can_communicate() {
            return Err(FirecrackerError::Io {
                message: "guest channel requested before start".to_owned(),
            });
        }
        GuestChannel::connect(
            self.config.vsock_uds_path(),
            self.config.vsock().port(),
            crate::vsock::DEFAULT_READ_TIMEOUT,
        )
    }

    /// Reclaims the microVM. Idempotent and safe to call more than once.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError`] only when the process kill fails.
    pub async fn terminate(&mut self) -> Result<(), FirecrackerError> {
        if self.lifecycle.phase() == VmPhase::Terminated {
            return Ok(());
        }
        self.lifecycle
            .teardown()
            .map_err(|phase| FirecrackerError::Io {
                message: format!("cannot tear down from {phase}"),
            })?;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let _ = tokio::fs::remove_file(&self.api_socket).await;
        self.lifecycle.reclaimed();
        Ok(())
    }
}

impl Drop for FirecrackerVm {
    fn drop(&mut self) {
        // Best-effort synchronous teardown if not already reclaimed.
        if self.lifecycle.phase().needs_teardown() {
            if let Some(mut child) = self.child.take() {
                let _ = child.start_kill();
            }
            let _ = std::fs::remove_file(&self.api_socket);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FirecrackerConfigParts, MachineConfig, ResourceLimits, VsockConfig};

    fn fixture_config() -> Result<FirecrackerConfig, FirecrackerError> {
        FirecrackerConfig::from_parts(FirecrackerConfigParts {
            kernel_path: "/tmp/kernel.bin".into(),
            kernel_digest: "sha256:k".into(),
            rootfs_path: "/tmp/rootfs.ext4".into(),
            rootfs_digest: "sha256:r".into(),
            boot_args: "console=ttyS0".into(),
            machine: MachineConfig::new(1, 128)?,
            limits: ResourceLimits::default(),
            vsock: VsockConfig::new(3, 8266)?,
            vsock_uds_path: "/tmp/vsock.sock".into(),
            jailer_path: None,
        })
    }

    #[test]
    fn new_vm_is_idle() -> Result<(), Box<dyn std::error::Error>> {
        let vm = FirecrackerVm::new(fixture_config()?, "/usr/bin/firecracker", "/tmp/fc-1.sock");
        assert!(!vm.can_communicate());
        Ok(())
    }

    #[tokio::test]
    async fn terminate_on_idle_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let mut vm = FirecrackerVm::new(
            fixture_config()?,
            "/usr/bin/firecracker",
            "/tmp/fc-idle.sock",
        );
        vm.terminate().await?;
        vm.terminate().await?;
        assert_eq!(vm.phase(), VmPhase::Terminated);
        Ok(())
    }
}
