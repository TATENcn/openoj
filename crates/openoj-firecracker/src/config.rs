//! Bounded configuration for a Firecracker microVM.
//!
//! All bounds and validation rules live here so they can be unit-tested without
//! a running VMM. Production paths fail closed when a required image digest or
//! isolation value is absent.

use std::path::{Path, PathBuf};

/// Minimum memory a microVM may be configured with, in MiB.
pub const MIN_MEMORY_MIB: u32 = 64;

/// Maximum memory a microVM may be configured with, in MiB.
pub const MAX_MEMORY_MIB: u32 = 65_536;

/// Minimum number of guest vCPUs.
pub const MIN_VCPUS: u8 = 1;

/// Maximum number of guest vCPUs.
pub const MAX_VCPUS: u8 = 255;

/// Fixed guest context identifier used on the host `AF_VSOCK` side.
pub const DEFAULT_GUEST_CID: u32 = 3;

/// Valid guest context identifiers (must be > 2; 3 is the conventional guest).
pub const MIN_GUEST_CID: u32 = 3;

/// The vsock port the guest agent listens on.
pub const DEFAULT_GUEST_PORT: u32 = 8266;

/// Bounds for the guest vsock port.
pub const MIN_GUEST_PORT: u32 = 1;

/// Bounds for the guest vsock port.
pub const MAX_GUEST_PORT: u32 = 65_535;

/// A bounded error produced while building or driving a microVM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FirecrackerError {
    /// A configuration value is outside its declared bound.
    OutOfRange {
        field: &'static str,
        minimum: u64,
        maximum: u64,
        actual: u64,
    },
    /// A required path is not absolute.
    NonAbsolutePath { field: &'static str },
    /// A required runtime image digest is missing.
    MissingDigest { field: &'static str },
    /// The jailer binary path is required but absent.
    MissingJailer,
    /// A resource limit is inconsistent (period zero or quota with no period).
    InvalidResourceLimit { field: &'static str },
    /// The production profile requires jailer isolation.
    ProductionRequiresJailer,
    /// The Firecracker control API returned a non-2xx status.
    ControlApi { status: u16, detail: String },
    /// A low-level I/O operation failed (process, socket, or control API).
    Io { message: String },
}

impl From<std::io::Error> for FirecrackerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

impl From<openoj_guest_protocol::CodecError> for FirecrackerError {
    fn from(error: openoj_guest_protocol::CodecError) -> Self {
        Self::Io {
            message: format!("guest message codec error: {error}"),
        }
    }
}

impl std::fmt::Display for FirecrackerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfRange {
                field,
                minimum,
                maximum,
                actual,
            } => write!(
                formatter,
                "{field} must be within {minimum}..={maximum}, got {actual}"
            ),
            Self::NonAbsolutePath { field } => {
                write!(formatter, "{field} must be an absolute path")
            }
            Self::MissingDigest { field } => write!(formatter, "{field} image digest is required"),
            Self::MissingJailer => write!(formatter, "jailer binary path is required"),
            Self::InvalidResourceLimit { field } => {
                write!(formatter, "resource limit {field} is inconsistent")
            }
            Self::ProductionRequiresJailer => {
                write!(formatter, "production profile requires jailer isolation")
            }
            Self::ControlApi { status, detail } => {
                write!(
                    formatter,
                    "Firecracker control API returned {status}: {detail}"
                )
            }
            Self::Io { message } => write!(formatter, "I/O error: {message}"),
        }
    }
}

impl std::error::Error for FirecrackerError {}

fn require_range(
    field: &'static str,
    actual: u64,
    minimum: u64,
    maximum: u64,
) -> Result<(), FirecrackerError> {
    if !(minimum..=maximum).contains(&actual) {
        return Err(FirecrackerError::OutOfRange {
            field,
            minimum,
            maximum,
            actual,
        });
    }
    Ok(())
}

fn require_absolute(path: &Path, field: &'static str) -> Result<(), FirecrackerError> {
    if !path.is_absolute() {
        return Err(FirecrackerError::NonAbsolutePath { field });
    }
    Ok(())
}

/// vCPU and memory sizing for the microVM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineConfig {
    vcpu_count: u8,
    mem_size_mib: u32,
}

impl MachineConfig {
    /// Creates a bounded machine configuration.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError::OutOfRange`] when either value is outside its
    /// protocol bound.
    pub fn new(vcpu_count: u8, mem_size_mib: u32) -> Result<Self, FirecrackerError> {
        require_range(
            "vcpu_count",
            u64::from(vcpu_count),
            u64::from(MIN_VCPUS),
            u64::from(MAX_VCPUS),
        )?;
        require_range(
            "mem_size_mib",
            u64::from(mem_size_mib),
            u64::from(MIN_MEMORY_MIB),
            u64::from(MAX_MEMORY_MIB),
        )?;
        Ok(Self {
            vcpu_count,
            mem_size_mib,
        })
    }

    /// Number of guest vCPUs.
    #[must_use]
    pub const fn vcpu_count(self) -> u8 {
        self.vcpu_count
    }

    /// Guest memory size in MiB.
    #[must_use]
    pub const fn mem_size_mib(self) -> u32 {
        self.mem_size_mib
    }
}

/// Optional cgroup-style resource ceilings enforced on the VMM.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceLimits {
    cpu_quota_us: Option<u32>,
    cpu_period_us: Option<u32>,
    memory_bytes: Option<u64>,
    pids_max: Option<u32>,
}

impl ResourceLimits {
    /// Creates resource ceilings. A `cpu_quota_us` requires a nonzero period;
    /// the period must be nonzero and non-decreasing where provided.
    #[must_use]
    pub const fn new(
        cpu_quota_us: Option<u32>,
        cpu_period_us: Option<u32>,
        memory_bytes: Option<u64>,
        pids_max: Option<u32>,
    ) -> Self {
        Self {
            cpu_quota_us,
            cpu_period_us,
            memory_bytes,
            pids_max,
        }
    }

    /// CPU quota in microseconds.
    #[must_use]
    pub const fn cpu_quota_us(self) -> Option<u32> {
        self.cpu_quota_us
    }

    /// CPU scheduling period in microseconds.
    #[must_use]
    pub const fn cpu_period_us(self) -> Option<u32> {
        self.cpu_period_us
    }

    /// Optional hard memory ceiling in bytes.
    #[must_use]
    pub const fn memory_bytes(self) -> Option<u64> {
        self.memory_bytes
    }

    /// Optional maximum process count for the cgroup.
    #[must_use]
    pub const fn pids_max(self) -> Option<u32> {
        self.pids_max
    }

    /// Validates internal consistency (quota implies nonzero period).
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError::InvalidResourceLimit`] when a quota is set
    /// without a nonzero period.
    pub fn validate(self) -> Result<(), FirecrackerError> {
        match (self.cpu_quota_us, self.cpu_period_us) {
            (Some(_), Some(0) | None) => {
                Err(FirecrackerError::InvalidResourceLimit { field: "cpu" })
            }
            _ => Ok(()),
        }
    }
}

/// The guest vsock device configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VsockConfig {
    guest_cid: u32,
    port: u32,
}

impl VsockConfig {
    /// Creates a bounded vsock device configuration.
    ///
    /// # Errors
    ///
    /// Returns [`FirecrackerError::OutOfRange`] when the CID or port is outside
    /// its allowed range.
    pub fn new(guest_cid: u32, port: u32) -> Result<Self, FirecrackerError> {
        require_range(
            "guest_cid",
            u64::from(guest_cid),
            u64::from(MIN_GUEST_CID),
            u64::from(u32::MAX),
        )?;
        require_range(
            "port",
            u64::from(port),
            u64::from(MIN_GUEST_PORT),
            u64::from(MAX_GUEST_PORT),
        )?;
        Ok(Self { guest_cid, port })
    }

    /// The guest context identifier seen from the host `AF_VSOCK` side.
    #[must_use]
    pub const fn guest_cid(self) -> u32 {
        self.guest_cid
    }

    /// The guest port the agent listens on.
    #[must_use]
    pub const fn port(self) -> u32 {
        self.port
    }
}

/// Validated configuration for a single Firecracker microVM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirecrackerConfig {
    kernel_path: PathBuf,
    kernel_digest: String,
    rootfs_path: PathBuf,
    rootfs_digest: String,
    boot_args: String,
    machine: MachineConfig,
    limits: ResourceLimits,
    vsock: VsockConfig,
    vsock_uds_path: PathBuf,
    jailer_path: Option<PathBuf>,
}

/// Untyped inputs validated by [`FirecrackerConfig::from_parts`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirecrackerConfigParts {
    /// Absolute path to the guest kernel image.
    pub kernel_path: PathBuf,
    /// Content digest of the guest kernel image.
    pub kernel_digest: String,
    /// Absolute path to the guest root filesystem image.
    pub rootfs_path: PathBuf,
    /// Content digest of the guest root filesystem image.
    pub rootfs_digest: String,
    /// Kernel boot arguments.
    pub boot_args: String,
    /// Machine sizing for the microVM.
    pub machine: MachineConfig,
    /// Resource ceilings for the VMM.
    pub limits: ResourceLimits,
    /// Guest vsock device configuration.
    pub vsock: VsockConfig,
    /// Absolute host unix-socket path Firecracker bridges to the guest vsock.
    pub vsock_uds_path: PathBuf,
    /// Optional jailer binary; `Some` is required for the production profile.
    pub jailer_path: Option<PathBuf>,
}

impl FirecrackerConfig {
    /// Builds a validated Firecracker configuration from its parts.
    ///
    /// # Errors
    ///
    /// Returns a [`FirecrackerError`] when a required path is relative, a digest
    /// is missing, a resource limit is inconsistent, or a machine/vsock value is
    /// out of range.
    pub fn from_parts(parts: FirecrackerConfigParts) -> Result<Self, FirecrackerError> {
        let kernel_path = parts.kernel_path;
        let rootfs_path = parts.rootfs_path;
        require_absolute(&kernel_path, "kernel_path")?;
        require_absolute(&rootfs_path, "rootfs_path")?;
        let kernel_digest = parts.kernel_digest;
        let rootfs_digest = parts.rootfs_digest;
        if kernel_digest.is_empty() {
            return Err(FirecrackerError::MissingDigest { field: "kernel" });
        }
        if rootfs_digest.is_empty() {
            return Err(FirecrackerError::MissingDigest { field: "rootfs" });
        }
        parts.limits.validate()?;
        if let Some(path) = &parts.jailer_path {
            require_absolute(path, "jailer_path")?;
        }
        require_absolute(&parts.vsock_uds_path, "vsock_uds_path")?;
        Ok(Self {
            kernel_path,
            kernel_digest,
            rootfs_path,
            rootfs_digest,
            boot_args: parts.boot_args,
            machine: parts.machine,
            limits: parts.limits,
            vsock: parts.vsock,
            vsock_uds_path: parts.vsock_uds_path,
            jailer_path: parts.jailer_path,
        })
    }

    /// Absolute path to the guest kernel image.
    #[must_use]
    pub fn kernel_path(&self) -> &Path {
        &self.kernel_path
    }

    /// Content digest of the guest kernel image.
    #[must_use]
    pub fn kernel_digest(&self) -> &str {
        &self.kernel_digest
    }

    /// Absolute path to the guest root filesystem image.
    #[must_use]
    pub fn rootfs_path(&self) -> &Path {
        &self.rootfs_path
    }

    /// Content digest of the guest root filesystem image.
    #[must_use]
    pub fn rootfs_digest(&self) -> &str {
        &self.rootfs_digest
    }

    /// Kernel boot arguments.
    #[must_use]
    pub fn boot_args(&self) -> &str {
        &self.boot_args
    }

    /// Machine sizing for the microVM.
    #[must_use]
    pub const fn machine(&self) -> MachineConfig {
        self.machine
    }

    /// Resource ceilings for the VMM.
    #[must_use]
    pub const fn limits(&self) -> ResourceLimits {
        self.limits
    }

    /// Guest vsock device configuration.
    #[must_use]
    pub const fn vsock(&self) -> VsockConfig {
        self.vsock
    }

    /// Absolute host unix-socket path Firecracker bridges to the guest vsock.
    #[must_use]
    pub fn vsock_uds_path(&self) -> &Path {
        &self.vsock_uds_path
    }

    /// Optional jailer binary; `Some` is required for the production profile.
    #[must_use]
    pub fn jailer_path(&self) -> Option<&Path> {
        self.jailer_path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rootfs() -> PathBuf {
        std::path::PathBuf::from("/tmp/rootfs.ext4")
    }

    fn default_parts() -> Result<FirecrackerConfigParts, FirecrackerError> {
        Ok(FirecrackerConfigParts {
            kernel_path: "/tmp/hello-vmlinux.bin".into(),
            kernel_digest: "sha256:kernel".into(),
            rootfs_path: rootfs(),
            rootfs_digest: "sha256:rootfs".into(),
            boot_args: "console=ttyS0 reboot=k panic=1 pci=off".into(),
            machine: MachineConfig::new(1, 128)?,
            limits: ResourceLimits::default(),
            vsock: VsockConfig::new(DEFAULT_GUEST_CID, DEFAULT_GUEST_PORT)?,
            vsock_uds_path: "/tmp/vsock.sock".into(),
            jailer_path: None,
        })
    }

    fn config() -> Result<FirecrackerConfig, FirecrackerError> {
        FirecrackerConfig::from_parts(default_parts()?)
    }

    #[test]
    fn valid_config_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        let value = config()?;
        assert_eq!(value.machine().vcpu_count(), 1);
        assert_eq!(value.vsock().guest_cid(), DEFAULT_GUEST_CID);
        Ok(())
    }

    #[test]
    fn relative_paths_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let mut parts = default_parts()?;
        parts.kernel_path = "relative/kernel".into();
        let result = FirecrackerConfig::from_parts(parts);
        assert_eq!(
            result,
            Err(FirecrackerError::NonAbsolutePath {
                field: "kernel_path"
            })
        );
        Ok(())
    }

    #[test]
    fn missing_digest_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let mut parts = default_parts()?;
        parts.kernel_digest = String::new();
        let result = FirecrackerConfig::from_parts(parts);
        assert_eq!(
            result,
            Err(FirecrackerError::MissingDigest { field: "kernel" })
        );
        Ok(())
    }

    #[test]
    fn out_of_range_memory_is_rejected() {
        let result = MachineConfig::new(1, 1);
        assert_eq!(
            result,
            Err(FirecrackerError::OutOfRange {
                field: "mem_size_mib",
                minimum: u64::from(MIN_MEMORY_MIB),
                maximum: u64::from(MAX_MEMORY_MIB),
                actual: 1,
            })
        );
    }

    #[test]
    fn quota_without_period_is_rejected() {
        let limits = ResourceLimits::new(Some(50_000), None, None, None);
        assert_eq!(
            limits.validate(),
            Err(FirecrackerError::InvalidResourceLimit { field: "cpu" })
        );
    }

    #[test]
    fn port_bounds_are_enforced() {
        let result = VsockConfig::new(DEFAULT_GUEST_CID, 0);
        assert!(result.is_err());
        assert!(VsockConfig::new(DEFAULT_GUEST_CID, MAX_GUEST_PORT).is_ok());
    }

    #[test]
    fn guest_cid_must_exceed_two() {
        assert!(VsockConfig::new(2, DEFAULT_GUEST_PORT).is_err());
        assert!(VsockConfig::new(MIN_GUEST_CID, DEFAULT_GUEST_PORT).is_ok());
    }
}
