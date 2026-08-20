//! `openoj-firecracker`: the platform/privileged jailer + VMM adapter.
//!
//! This crate is the *only* component authorized to own Firecracker/jailer
//! processes, control APIs, vsock channels, and privileged resource recovery. It
//! contains no user, competition, or scoring logic. All unsafe platform access is
//! delegated to trusted libraries; this crate itself stays `forbid(unsafe_code)`.

pub mod api;
pub mod config;
pub mod lifecycle;
pub mod machine;
pub mod vsock;

pub use config::{
    FirecrackerConfig, FirecrackerConfigParts, FirecrackerError, MachineConfig, ResourceLimits,
    VsockConfig,
};
pub use lifecycle::{Lifecycle, TeardownReason, VmPhase};
pub use machine::FirecrackerVm;
pub use vsock::GuestChannel;
