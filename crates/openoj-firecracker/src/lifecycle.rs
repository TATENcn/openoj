//! Bounded microVM lifecycle state machine.
//!
//! The state machine guarantees a single VMM owned per task, an idempotent
//! teardown, and deterministic terminal states. It is intentionally small and
//! transport-free so it can be unit-tested without a running VMM.

use std::fmt::{self, Display, Formatter};

/// The lifecycle phase of a single microVM for one task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmPhase {
    /// No VMM has been launched yet.
    Idle,
    /// The firecracker/jailer process is starting.
    Launching,
    /// The Firecracker control API is being configured.
    Configuring,
    /// The microVM has booted (instance started).
    Started,
    /// Guest build/run stages are executing.
    Running,
    /// The instance is being terminated; teardown is in progress.
    Terminating,
    /// The instance has been fully reclaimed. Terminal.
    Terminated,
    /// The instance failed and can be reclaimed. Terminal.
    Failed,
}

impl VmPhase {
    /// Whether this phase is reachable as a normal lifecycle step.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminated | Self::Failed)
    }

    /// Whether the process may still own host resources and must be reclaimed.
    #[must_use]
    pub const fn needs_teardown(self) -> bool {
        matches!(
            self,
            Self::Launching | Self::Configuring | Self::Started | Self::Running | Self::Terminating
        )
    }

    /// Whether a vsock guest channel may be opened from this phase.
    #[must_use]
    pub const fn can_communicate(self) -> bool {
        matches!(self, Self::Started | Self::Running)
    }
}

impl Display for VmPhase {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Idle => "idle",
            Self::Launching => "launching",
            Self::Configuring => "configuring",
            Self::Started => "started",
            Self::Running => "running",
            Self::Terminating => "terminating",
            Self::Terminated => "terminated",
            Self::Failed => "failed",
        };
        formatter.write_str(name)
    }
}

/// A failure reason attached to the `Failed` terminal phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TeardownReason {
    /// The process exited unexpectedly before completion.
    UnexpectedExit,
    /// A control API request to the VMM failed.
    ControlApi,
    /// A guest stage failed deterministically.
    GuestStage,
    /// The caller requested cancellation.
    Cancelled,
    /// A resource limit was exceeded.
    ResourceLimit,
}

/// The lifecycle of a single microVM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lifecycle {
    phase: VmPhase,
    /// Number of times teardown has been requested; guards idempotency.
    teardowns: u32,
    reason: Option<TeardownReason>,
}

impl Lifecycle {
    /// Creates a new idle lifecycle.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: VmPhase::Idle,
            teardowns: 0,
            reason: None,
        }
    }

    /// Returns the current lifecycle phase.
    #[must_use]
    pub const fn phase(&self) -> VmPhase {
        self.phase
    }

    /// Returns the teardown failure reason once terminal.
    #[must_use]
    pub const fn reason(&self) -> Option<TeardownReason> {
        self.reason
    }

    /// Advances to `Launching` from `Idle`.
    ///
    /// # Errors
    ///
    /// Returns the current phase when the transition is not allowed.
    pub fn launch(&mut self) -> Result<(), VmPhase> {
        self.transition(VmPhase::Launching)
    }

    /// Advances to `Configuring` from `Launching`.
    ///
    /// # Errors
    ///
    /// Returns the current phase when the transition is not allowed.
    pub fn configuring(&mut self) -> Result<(), VmPhase> {
        if self.phase != VmPhase::Launching {
            return Err(self.phase);
        }
        self.phase = VmPhase::Configuring;
        Ok(())
    }

    /// Advances to `Started` from `Configuring`.
    ///
    /// # Errors
    ///
    /// Returns the current phase when the transition is not allowed.
    pub fn start(&mut self) -> Result<(), VmPhase> {
        if self.phase != VmPhase::Configuring {
            return Err(self.phase);
        }
        self.phase = VmPhase::Started;
        Ok(())
    }

    /// Advances to `Running` from `Started`.
    ///
    /// # Errors
    ///
    /// Returns the current phase when the transition is not allowed.
    pub fn running(&mut self) -> Result<(), VmPhase> {
        if self.phase != VmPhase::Started {
            return Err(self.phase);
        }
        self.phase = VmPhase::Running;
        Ok(())
    }

    fn transition(&mut self, target: VmPhase) -> Result<(), VmPhase> {
        const ALLOWED: &[(VmPhase, VmPhase)] = &[
            (VmPhase::Idle, VmPhase::Launching),
            (VmPhase::Launching, VmPhase::Configuring),
            (VmPhase::Configuring, VmPhase::Started),
            (VmPhase::Started, VmPhase::Running),
            (VmPhase::Running, VmPhase::Terminating),
            (VmPhase::Started, VmPhase::Terminating),
            (VmPhase::Configuring, VmPhase::Terminating),
            (VmPhase::Launching, VmPhase::Terminating),
        ];
        if !ALLOWED.contains(&(self.phase, target)) {
            return Err(self.phase);
        }
        self.phase = target;
        Ok(())
    }

    /// Fails the lifecycle and records a reason.
    fn fail(&mut self, reason: TeardownReason) {
        self.phase = VmPhase::Failed;
        self.reason = Some(reason);
    }

    /// Requests teardown. Repeated calls are idempotent and return `Ok`.
    ///
    /// # Errors
    ///
    /// Returns the current phase when teardown is requested from an unknown
    /// terminal state (e.g. after `Terminated`).
    pub fn teardown(&mut self) -> Result<(), VmPhase> {
        match self.phase {
            VmPhase::Idle => {
                // Nothing to reclaim; the instance never started.
                self.teardowns += 1;
                self.phase = VmPhase::Terminated;
                Ok(())
            }
            VmPhase::Launching
            | VmPhase::Configuring
            | VmPhase::Started
            | VmPhase::Running => {
                self.teardowns += 1;
                self.phase = VmPhase::Terminating;
                Ok(())
            }
            VmPhase::Terminating => {
                // Already tearing down; idempotent.
                self.teardowns += 1;
                Ok(())
            }
            VmPhase::Terminated => Err(VmPhase::Terminated),
            VmPhase::Failed => Err(VmPhase::Failed),
        }
    }

    /// Marks teardown as complete, producing the `Terminated` terminal phase.
    pub fn reclaimed(&mut self) {
        self.phase = VmPhase::Terminated;
    }

    /// Marks the lifecycle as failed with the given reason.
    pub fn fail_with(&mut self, reason: TeardownReason) {
        self.fail(reason);
    }

    /// Total number of teardown requests received (for idempotency accounting).
    #[must_use]
    pub const fn teardowns(&self) -> u32 {
        self.teardowns
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_progression_reaches_running() -> Result<(), VmPhase> {
        let mut lifecycle = Lifecycle::new();
        assert_eq!(lifecycle.phase(), VmPhase::Idle);
        lifecycle.launch()?;
        lifecycle.configuring()?;
        lifecycle.start()?;
        lifecycle.running()?;
        assert_eq!(lifecycle.phase(), VmPhase::Running);
        assert!(lifecycle.phase().can_communicate());
        Ok(())
    }

    #[test]
    fn cannot_skip_starting_phase() -> Result<(), VmPhase> {
        let mut lifecycle = Lifecycle::new();
        lifecycle.launch()?;
        assert_eq!(lifecycle.start(), Err(VmPhase::Launching));
        Ok(())
    }

    #[test]
    fn teardown_is_idempotent_from_running() -> Result<(), VmPhase> {
        let mut lifecycle = Lifecycle::new();
        lifecycle.launch()?;
        lifecycle.configuring()?;
        lifecycle.start()?;
        lifecycle.running()?;

        lifecycle.teardown()?;
        assert!(lifecycle.phase().needs_teardown());
        lifecycle.teardown()?;
        assert_eq!(lifecycle.teardowns(), 2);
        lifecycle.reclaimed();
        assert_eq!(lifecycle.phase(), VmPhase::Terminated);
        assert!(lifecycle.phase().is_terminal());
        assert!(!lifecycle.phase().needs_teardown());
        Ok(())
    }

    #[test]
    fn idle_teardown_terminates_without_launch() -> Result<(), VmPhase> {
        let mut lifecycle = Lifecycle::new();
        lifecycle.teardown()?;
        assert_eq!(lifecycle.phase(), VmPhase::Terminated);
        Ok(())
    }

    #[test]
    fn failed_is_terminal_and_not_reclaimable_twice() {
        let mut lifecycle = Lifecycle::new();
        lifecycle.fail_with(TeardownReason::ControlApi);
        assert_eq!(lifecycle.phase(), VmPhase::Failed);
        assert!(lifecycle.phase().is_terminal());
        assert!(!lifecycle.phase().needs_teardown());
        assert!(!lifecycle.phase().can_communicate());
    }

    #[test]
    fn terminated_teardown_is_rejected() -> Result<(), VmPhase> {
        let mut lifecycle = Lifecycle::new();
        lifecycle.teardown()?;
        assert_eq!(lifecycle.teardown(), Err(VmPhase::Terminated));
        Ok(())
    }
}
