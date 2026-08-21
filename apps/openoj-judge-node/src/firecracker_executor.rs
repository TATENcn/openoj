//! Real Firecracker executor for a judge node.
//!
//! [`FirecrackerExecutor`] drives a [`openoj_firecracker::FirecrackerVm`] and its
//! guest channel through the canonical stage plan, mapping guest
//! [`openoj_guest_protocol::Message::StageOutput`] events into domain
//! [`openoj_application::StageExecution`]. It fails closed: the `production`
//! profile refuses to run without a configured jailer, and a development mock is
//! never selected in a production configuration.
//!
//! The executor talks to the guest through the [`GuestSession`] trait so the
//! stage orchestration can be unit-tested with a fake session, while a real
//! [`FirecrackerGuestSession`] drives the actual microVM over vsock.

use std::path::PathBuf;

use openoj_application::{ApplicationError, StageContext, StageExecution, StageExecutor};
use openoj_domain::{
    Capability, EvaluationRequest, EvaluationResult, ExecutorKind, NodeId, ResourceUsage, StageKind,
};
use openoj_evaluator::{execution_for_stage_output, host_check_decision};
use openoj_firecracker::{FirecrackerConfig, FirecrackerError, FirecrackerVm, GuestChannel};
use openoj_guest_protocol::Message;
use openoj_judge_core::JudgeExecutor;

/// Default build stage wall-clock budget in milliseconds.
pub const BUILD_WALL_MS: u64 = 30_000;

/// Default run stage wall-clock budget in milliseconds.
pub const RUN_WALL_MS: u64 = 10_000;

/// A bounded, guest-facing session used by the executor to run guest stages.
pub trait GuestSession: Send {
    /// Sends one bounded host→guest message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] when the transport or framing fails.
    fn send(&mut self, message: &Message) -> Result<(), ApplicationError>;

    /// Receives one bounded guest→host message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] when the transport, bound, or framing fails.
    fn recv(&mut self) -> Result<Message, ApplicationError>;
}

/// A [`GuestSession`] backed by a real Firecracker vsock channel.
pub struct FirecrackerGuestSession {
    channel: GuestChannel,
}

impl FirecrackerGuestSession {
    /// Opens a session on an already-connected guest channel.
    #[must_use]
    pub const fn new(channel: GuestChannel) -> Self {
        Self { channel }
    }
}

impl GuestSession for FirecrackerGuestSession {
    fn send(&mut self, message: &Message) -> Result<(), ApplicationError> {
        self.channel.send(message).map_err(map_firecracker)
    }

    fn recv(&mut self) -> Result<Message, ApplicationError> {
        self.channel.recv().map_err(map_firecracker)
    }
}

fn map_firecracker(_error: FirecrackerError) -> ApplicationError {
    ApplicationError::InvalidExecutorOutput {
        reason: "guest transport failed",
    }
}

/// Configuration required to assemble a Firecracker executor.
#[derive(Clone, Debug)]
pub struct FirecrackerExecutorConfig {
    /// Node identity recorded in result provenance.
    pub node_id: NodeId,
    /// Absolute path to the `firecracker` binary.
    pub firecracker_path: PathBuf,
    /// Absolute path to the Firecracker control unix socket.
    pub api_socket: PathBuf,
    /// Validated microVM configuration.
    pub vm_config: FirecrackerConfig,
    /// Whether the production profile is requested (requires a jailer).
    pub production: bool,
}

impl FirecrackerExecutorConfig {
    /// Validates fail-closed invariants: the production profile requires a jailer.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] when the production profile has no jailer.
    pub fn validate(&self) -> Result<(), ApplicationError> {
        if self.production && self.vm_config.jailer_path().is_none() {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "production requires jailer isolation",
            });
        }
        Ok(())
    }
}

/// A real Firecracker executor that runs the canonical algorithm-batch plan.
pub struct FirecrackerExecutor {
    config: FirecrackerExecutorConfig,
    runtime: tokio::runtime::Runtime,
    session: Option<Box<dyn GuestSession>>,
    negotiated: bool,
    last_run_exit: Option<i32>,
    terminated: bool,
}

impl FirecrackerExecutor {
    /// Assembles an executor, building a single-threaded async runtime for the VMM.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] when the config is invalid or the runtime
    /// cannot be built.
    pub fn try_new(config: FirecrackerExecutorConfig) -> Result<Self, ApplicationError> {
        config.validate()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ApplicationError::InvalidExecutorOutput {
                reason: "firecracker runtime unavailable",
            })?;
        Ok(Self {
            config,
            runtime,
            session: None,
            negotiated: false,
            last_run_exit: None,
            terminated: false,
        })
    }

    fn ensure_session(&mut self) -> Result<(), ApplicationError> {
        if self.session.is_some() {
            return Ok(());
        }
        let mut vm = FirecrackerVm::new(
            self.config.vm_config.clone(),
            self.config.firecracker_path.clone(),
            self.config.api_socket.clone(),
        );
        self.runtime
            .block_on(vm.bootstrap(self.config.production))
            .map_err(|_| ApplicationError::InvalidExecutorOutput {
                reason: "microvm bootstrap failed",
            })?;
        let channel =
            vm.open_guest_channel()
                .map_err(|_| ApplicationError::InvalidExecutorOutput {
                    reason: "guest channel unavailable",
                })?;
        self.session = Some(Box::new(FirecrackerGuestSession::new(channel)));
        Ok(())
    }

    fn ensure_negotiated(&mut self) -> Result<(), ApplicationError> {
        if self.negotiated {
            return Ok(());
        }
        let session = self
            .session
            .as_mut()
            .ok_or(ApplicationError::UnsupportedCapability)?;
        session.send(&Message::Negotiate {
            capabilities: vec!["algorithm.batch".to_owned()],
        })?;
        match session.recv()? {
            Message::Negotiated { supported: true } => {
                self.negotiated = true;
                Ok(())
            }
            Message::Negotiated { supported: false } => {
                Err(ApplicationError::UnsupportedCapability)
            }
            _ => Err(ApplicationError::InvalidExecutorOutput {
                reason: "unexpected negotiate reply",
            }),
        }
    }

    fn run_guest_stage(
        &mut self,
        stage: StageKind,
        argv: &[String],
        wall_time_ms: u64,
    ) -> Result<StageExecution, ApplicationError> {
        self.ensure_session()?;
        self.ensure_negotiated()?;
        let session = self
            .session
            .as_mut()
            .ok_or(ApplicationError::UnsupportedCapability)?;
        let message = match stage {
            StageKind::Build => Message::Build {
                argv: argv.to_vec(),
                wall_time_ms,
            },
            _ => Message::Run {
                argv: argv.to_vec(),
                wall_time_ms,
            },
        };
        session.send(&message)?;
        let reply = session.recv()?;
        let Message::StageOutput {
            exit_code,
            output_digest,
            output_bytes,
            usage,
            diagnostics,
            ..
        } = reply
        else {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "expected stage output",
            });
        };
        if stage == StageKind::Run {
            self.last_run_exit = Some(exit_code);
        }
        execution_for_stage_output(
            stage,
            exit_code,
            &output_digest,
            output_bytes,
            usage,
            &diagnostics,
        )
    }

    /// Releases the guest session; safe to call more than once.
    pub fn teardown(&mut self) {
        self.session.take();
        self.terminated = true;
    }
}

impl Drop for FirecrackerExecutor {
    fn drop(&mut self) {
        self.teardown();
    }
}

impl JudgeExecutor for FirecrackerExecutor {
    fn execute(
        &mut self,
        request: &EvaluationRequest,
    ) -> Result<EvaluationResult, ApplicationError> {
        openoj_application::evaluate(request, self)
    }
}

impl StageExecutor for FirecrackerExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Firecracker
    }

    fn production_eligible(&self) -> bool {
        self.config.production
    }

    fn node_id(&self) -> Option<NodeId> {
        Some(self.config.node_id.clone())
    }

    fn supports(&self, capability: &Capability) -> bool {
        capability.as_str() == "algorithm.batch"
    }

    fn execute(&mut self, context: StageContext<'_>) -> StageExecution {
        if self.terminated {
            return stage_failed();
        }
        let outcome = match context.stage() {
            StageKind::Prepare | StageKind::Aggregate => Ok(StageExecution::Succeeded {
                usage: ResourceUsage::default(),
                diagnostics: Vec::new(),
                evidence: Vec::new(),
                decision: None,
            }),
            StageKind::Build => {
                self.run_guest_stage(StageKind::Build, &default_argv("build"), BUILD_WALL_MS)
            }
            StageKind::Run => {
                self.run_guest_stage(StageKind::Run, &default_argv("run"), RUN_WALL_MS)
            }
            StageKind::Check => match self.last_run_exit {
                Some(exit_code) => {
                    host_check_decision(exit_code).map(|decision| StageExecution::Succeeded {
                        usage: ResourceUsage::default(),
                        diagnostics: Vec::new(),
                        evidence: Vec::new(),
                        decision: Some(decision),
                    })
                }
                None => Err(ApplicationError::InvalidExecutorOutput {
                    reason: "check stage before run",
                }),
            },
        };
        outcome.unwrap_or_else(|_error| {
            self.teardown();
            stage_failed()
        })
    }
}

/// A fixed demonstration argv used until artifact storage delivers real commands.
fn default_argv(_kind: &str) -> Vec<String> {
    vec!["printf".to_owned(), "ok".to_owned()]
}

fn stage_failed() -> StageExecution {
    StageExecution::Failed {
        usage: ResourceUsage::default(),
        diagnostics: Vec::new(),
        evidence: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openoj_domain::Verdict;
    use openoj_guest_protocol::Stage as SessionStage;
    use openoj_judge_core::JudgeExecutor;
    use openoj_protocol::decode_evaluation_request;

    const DIGEST: &str = "abababababababababababababababababababababababababababababababab";
    const VALID_REQUEST: &[u8] =
        include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

    /// A scripted session that returns a successful negotiate/build/run.
    struct ScriptedSession {
        responses: Vec<Message>,
    }

    impl ScriptedSession {
        fn success() -> Self {
            Self {
                responses: vec![
                    Message::Negotiated { supported: true },
                    Message::StageOutput {
                        stage: SessionStage::Build,
                        exit_code: 0,
                        output_digest: DIGEST.to_owned(),
                        output_bytes: 2,
                        usage: openoj_guest_protocol::GuestUsage::default(),
                        diagnostics: Vec::new(),
                    },
                    Message::StageOutput {
                        stage: SessionStage::Run,
                        exit_code: 0,
                        output_digest: DIGEST.to_owned(),
                        output_bytes: 2,
                        usage: openoj_guest_protocol::GuestUsage::default(),
                        diagnostics: Vec::new(),
                    },
                ],
            }
        }
    }

    impl GuestSession for ScriptedSession {
        fn send(&mut self, _message: &Message) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn recv(&mut self) -> Result<Message, ApplicationError> {
            if self.responses.is_empty() {
                return Err(ApplicationError::InvalidExecutorOutput {
                    reason: "no more responses",
                });
            }
            Ok(self.responses.remove(0))
        }
    }

    fn vm_config() -> Result<openoj_firecracker::FirecrackerConfig, Box<dyn std::error::Error>> {
        Ok(openoj_firecracker::FirecrackerConfig::from_parts(
            openoj_firecracker::FirecrackerConfigParts {
                kernel_path: "/tmp/k".into(),
                kernel_digest: "sha256:k".into(),
                rootfs_path: "/tmp/r".into(),
                rootfs_digest: "sha256:r".into(),
                boot_args: "boot".into(),
                machine: openoj_firecracker::MachineConfig::new(1, 128)?,
                limits: openoj_firecracker::ResourceLimits::default(),
                vsock: openoj_firecracker::VsockConfig::new(3, 8266)?,
                vsock_uds_path: "/tmp/v.sock".into(),
                jailer_path: None,
            },
        )?)
    }

    fn executor_with(
        session: Box<dyn GuestSession>,
    ) -> Result<FirecrackerExecutor, Box<dyn std::error::Error>> {
        let mut executor = FirecrackerExecutor::try_new(FirecrackerExecutorConfig {
            node_id: NodeId::parse("judge_fc_01")?,
            firecracker_path: "/usr/bin/firecracker".into(),
            api_socket: "/tmp/fc.sock".into(),
            vm_config: vm_config()?,
            production: false,
        })?;
        executor.session = Some(session);
        Ok(executor)
    }

    #[test]
    fn firecracker_executor_accepts_successful_roundtrip() -> Result<(), Box<dyn std::error::Error>>
    {
        let request = decode_evaluation_request(VALID_REQUEST)?;
        let mut executor = executor_with(Box::new(ScriptedSession::success()))?;
        let result = JudgeExecutor::execute(&mut executor, &request)?;
        assert_eq!(result.verdict(), Verdict::Accepted);
        assert_eq!(result.provenance().executor_kind().as_str(), "firecracker");
        executor.teardown();
        Ok(())
    }

    #[test]
    fn production_without_jailer_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let result = FirecrackerExecutorConfig {
            node_id: NodeId::parse("judge_fc_01")?,
            firecracker_path: "/usr/bin/firecracker".into(),
            api_socket: "/tmp/fc.sock".into(),
            vm_config: vm_config()?,
            production: true,
        }
        .validate();
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn kind_is_firecracker() -> Result<(), Box<dyn std::error::Error>> {
        let executor = executor_with(Box::new(ScriptedSession::success()))?;
        assert_eq!(executor.kind(), ExecutorKind::Firecracker);
        assert!(!executor.production_eligible());
        Ok(())
    }
}
