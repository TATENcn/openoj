//! Real Firecracker executor for a judge node.
//!
//! [`FirecrackerExecutor`] drives a [`openoj_firecracker::FirecrackerVm`] and its
//! guest channel through the canonical stage plan, mapping guest
//! [`openoj_guest_protocol::Message::StageOutput`] events into domain
//! [`openoj_application::StageExecution`]. It fails closed: the current
//! Firecracker path is development-only until every mandatory production
//! isolation layer is implemented, and a development mock is never selected in
//! a production configuration.
//!
//! The executor talks to the guest through the [`GuestSession`] trait so the
//! stage orchestration can be unit-tested with a fake session, while a real
//! [`FirecrackerGuestSession`] drives the actual microVM over vsock.

use std::future::Future;
use std::path::PathBuf;

use openoj_application::{ApplicationError, StageContext, StageExecution, StageExecutor};
use openoj_domain::{
    Capability, ContentDigest, EvaluationRequest, EvaluationResult, ExecutorKind, NodeId,
    ResourceUsage, StageKind,
};
use openoj_evaluator::{execution_for_stage_output, host_check_decision};
use openoj_firecracker::{FirecrackerConfig, FirecrackerError, FirecrackerVm, GuestChannel};
use openoj_guest_protocol::{MAX_INLINE_BYTES, Message};
use openoj_judge_core::JudgeExecutor;
use sha2::{Digest, Sha256};

const ALGORITHM_C_SOURCE_NAME: &str = "main.c";
const ALGORITHM_C_SOURCE_MEDIA_TYPE: &str = "text/x-csrc";

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

fn block_on_vmm<F>(future: F) -> Result<F::Output, ApplicationError>
where
    F: Future,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                Ok(tokio::task::block_in_place(|| handle.block_on(future)))
            }
            _ => Err(ApplicationError::InvalidExecutorOutput {
                reason: "firecracker executor requires a multi-thread runtime",
            }),
        },
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ApplicationError::InvalidExecutorOutput {
                reason: "firecracker runtime unavailable",
            })
            .map(|runtime| runtime.block_on(future)),
    }
}

trait VmLease: Send {
    fn terminate(&mut self) -> Result<(), ApplicationError>;
}

struct FirecrackerVmLease {
    vm: FirecrackerVm,
}

impl VmLease for FirecrackerVmLease {
    fn terminate(&mut self) -> Result<(), ApplicationError> {
        block_on_vmm(self.vm.terminate())?.map_err(|_| ApplicationError::InvalidExecutorOutput {
            reason: "microvm teardown failed",
        })
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
    /// Fixed development artifact and host-side expected output.
    pub workload: AlgorithmCWorkload,
    /// Whether the production profile is requested.
    pub production: bool,
}

/// A bounded local-artifact bridge for the development-only algorithm-c slice.
///
/// The path used to load these bytes is deployment configuration and never
/// enters an Evaluation request. Production must replace this bridge with the
/// content-addressed Artifact store; [`FirecrackerExecutorConfig::validate`]
/// continues to reject the production profile.
#[derive(Clone, Debug)]
pub struct AlgorithmCWorkload {
    source: Vec<u8>,
    source_digest: ContentDigest,
    source_digest_hex: String,
    runtime_digest: ContentDigest,
    expected_output_digest: ContentDigest,
}

impl AlgorithmCWorkload {
    /// Creates a fixed workload after enforcing the guest inline-input bound.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] for an empty or oversized source artifact.
    pub fn new(
        source: Vec<u8>,
        runtime_digest: ContentDigest,
        expected_output_digest: ContentDigest,
    ) -> Result<Self, ApplicationError> {
        if source.is_empty() || source.len() > MAX_INLINE_BYTES {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "development source artifact is empty or oversized",
            });
        }
        let source_digest_hex = sha256_hex(&source);
        let source_digest = ContentDigest::parse(format!("sha256:{source_digest_hex}"))?;
        Ok(Self {
            source,
            source_digest,
            source_digest_hex,
            runtime_digest,
            expected_output_digest,
        })
    }

    fn validate_request(&self, request: &EvaluationRequest) -> Result<(), ApplicationError> {
        let source = request.submission().source();
        let size = u64::try_from(self.source.len()).map_err(|_| {
            ApplicationError::InvalidExecutorOutput {
                reason: "development source artifact size overflow",
            }
        })?;
        if source.digest() != &self.source_digest
            || source.size_bytes() != size
            || source.media_type().as_str() != ALGORITHM_C_SOURCE_MEDIA_TYPE
        {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "development source artifact does not match the request",
            });
        }
        if request.runtime().digest() != &self.runtime_digest {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "configured runtime digest does not match the request",
            });
        }
        Ok(())
    }

    fn source_digest_hex(&self) -> &str {
        &self.source_digest_hex
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl FirecrackerExecutorConfig {
    /// Validates fail-closed invariants for the incomplete production profile.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] whenever production is requested because
    /// uid/gid, cgroup, namespace, seccomp, and watchdog wiring is incomplete.
    pub fn validate(&self) -> Result<(), ApplicationError> {
        if self.production {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "production firecracker isolation profile is incomplete",
            });
        }
        Ok(())
    }
}

/// A real Firecracker executor that runs the canonical algorithm-batch plan.
pub struct FirecrackerExecutor {
    config: FirecrackerExecutorConfig,
    session: Option<Box<dyn GuestSession>>,
    vm: Option<Box<dyn VmLease>>,
    negotiated: bool,
    last_run_exit: Option<i32>,
    last_run_output_digest: Option<String>,
    terminated: bool,
}

impl FirecrackerExecutor {
    /// Assembles an executor after validating its fail-closed configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] when the config is invalid.
    pub fn try_new(config: FirecrackerExecutorConfig) -> Result<Self, ApplicationError> {
        config.validate()?;
        Ok(Self {
            config,
            session: None,
            vm: None,
            negotiated: false,
            last_run_exit: None,
            last_run_output_digest: None,
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
        if !matches!(
            block_on_vmm(vm.bootstrap(self.config.production)),
            Ok(Ok(()))
        ) {
            let _ = block_on_vmm(vm.terminate());
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "microvm bootstrap failed",
            });
        }
        let Ok(channel) = vm.open_guest_channel() else {
            let _ = block_on_vmm(vm.terminate());
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "guest channel unavailable",
            });
        };
        self.activate_session(
            Box::new(FirecrackerGuestSession::new(channel)),
            Box::new(FirecrackerVmLease { vm }),
        );
        Ok(())
    }

    fn activate_session(&mut self, session: Box<dyn GuestSession>, vm: Box<dyn VmLease>) {
        self.vm = Some(vm);
        self.session = Some(session);
    }

    fn begin_attempt(&mut self) {
        if self.terminated {
            self.negotiated = false;
            self.last_run_exit = None;
            self.last_run_output_digest = None;
            self.terminated = false;
        }
    }

    fn prepare(&mut self, request: &EvaluationRequest) -> Result<StageExecution, ApplicationError> {
        self.config.workload.validate_request(request)?;
        self.ensure_session()?;
        self.ensure_negotiated()?;
        let session = self
            .session
            .as_mut()
            .ok_or(ApplicationError::UnsupportedCapability)?;
        session.send(&Message::UploadInput {
            name: ALGORITHM_C_SOURCE_NAME.to_owned(),
            digest: self.config.workload.source_digest_hex().to_owned(),
            bytes: self.config.workload.source.clone(),
        })?;
        match session.recv()? {
            Message::UploadAck { name, accepted }
                if name == ALGORITHM_C_SOURCE_NAME && accepted =>
            {
                Ok(stage_succeeded())
            }
            Message::UploadAck { .. } => Err(ApplicationError::InvalidExecutorOutput {
                reason: "guest rejected the source artifact",
            }),
            _ => Err(ApplicationError::InvalidExecutorOutput {
                reason: "unexpected source upload reply",
            }),
        }
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
            self.last_run_output_digest = Some(output_digest.clone());
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
        if let Some(mut vm) = self.vm.take() {
            let _ = vm.terminate();
        }
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
        self.begin_attempt();
        let result = openoj_application::evaluate(request, self);
        self.teardown();
        result
    }
}

impl StageExecutor for FirecrackerExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Firecracker
    }

    fn production_eligible(&self) -> bool {
        false
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
            StageKind::Prepare => self.prepare(context.request()),
            StageKind::Aggregate => Ok(stage_succeeded()),
            StageKind::Build => self.run_guest_stage(
                StageKind::Build,
                &build_argv(),
                context.request().policy().wall_time_ms(),
            ),
            StageKind::Run => self.run_guest_stage(
                StageKind::Run,
                &run_argv(),
                context.request().policy().wall_time_ms(),
            ),
            StageKind::Check => match (&self.last_run_exit, &self.last_run_output_digest) {
                (Some(exit_code), Some(output_digest)) => host_check_decision(
                    *exit_code,
                    output_digest,
                    &self.config.workload.expected_output_digest,
                )
                .map(|decision| StageExecution::Succeeded {
                    usage: ResourceUsage::default(),
                    diagnostics: Vec::new(),
                    evidence: Vec::new(),
                    decision: Some(decision),
                }),
                _ => Err(ApplicationError::InvalidExecutorOutput {
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

fn build_argv() -> Vec<String> {
    [
        "/usr/bin/cc",
        "-std=c17",
        "-O2",
        "-o",
        "/work/main",
        "/work/inputs/main.c",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn run_argv() -> Vec<String> {
    vec!["/work/main".to_owned()]
}

fn stage_succeeded() -> StageExecution {
    StageExecution::Succeeded {
        usage: ResourceUsage::default(),
        diagnostics: Vec::new(),
        evidence: Vec::new(),
        decision: None,
    }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const DIGEST: &str = "abababababababababababababababababababababababababababababababab";
    const SOURCE: &[u8] = b"#include <stdio.h>\nint main(void) { puts(\"42\"); return 0; }\n";
    const SOURCE_DIGEST: &str = "c2329bffd207edd619d4ca7c7cdd872b76374e1a50421f705020671ca1f85556";
    const RUNTIME_DIGEST: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
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
                    Message::UploadAck {
                        name: ALGORITHM_C_SOURCE_NAME.to_owned(),
                        accepted: true,
                    },
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

    struct RecordingVmLease {
        teardowns: Arc<AtomicUsize>,
    }

    impl VmLease for RecordingVmLease {
        fn terminate(&mut self) -> Result<(), ApplicationError> {
            self.teardowns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn vm_config() -> Result<openoj_firecracker::FirecrackerConfig, Box<dyn std::error::Error>> {
        vm_config_with_jailer(None)
    }

    fn vm_config_with_jailer(
        jailer_path: Option<PathBuf>,
    ) -> Result<openoj_firecracker::FirecrackerConfig, Box<dyn std::error::Error>> {
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
                jailer_path,
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
            workload: workload()?,
            production: false,
        })?;
        executor.session = Some(session);
        Ok(executor)
    }

    fn workload() -> Result<AlgorithmCWorkload, Box<dyn std::error::Error>> {
        Ok(AlgorithmCWorkload::new(
            SOURCE.to_vec(),
            ContentDigest::parse(format!("sha256:{RUNTIME_DIGEST}"))?,
            ContentDigest::parse(format!("sha256:{DIGEST}"))?,
        )?)
    }

    fn request() -> Result<EvaluationRequest, Box<dyn std::error::Error>> {
        let value = String::from_utf8(VALID_REQUEST.to_vec())?
            .replace(&"2".repeat(64), SOURCE_DIGEST)
            .replace("text/x-c++src", ALGORITHM_C_SOURCE_MEDIA_TYPE)
            .replace("\"size_bytes\": 128", "\"size_bytes\": 60")
            .replace(&"3".repeat(64), RUNTIME_DIGEST);
        Ok(decode_evaluation_request(value.as_bytes())?)
    }

    #[test]
    fn workload_rejects_mismatched_artifact_before_boot() -> Result<(), Box<dyn std::error::Error>>
    {
        let mismatched = decode_evaluation_request(VALID_REQUEST)?;
        assert!(workload()?.validate_request(&mismatched).is_err());
        Ok(())
    }

    #[test]
    fn workload_rejects_an_oversized_inline_source() -> Result<(), Box<dyn std::error::Error>> {
        let result = AlgorithmCWorkload::new(
            vec![b'x'; MAX_INLINE_BYTES + 1],
            ContentDigest::parse(format!("sha256:{RUNTIME_DIGEST}"))?,
            ContentDigest::parse(format!("sha256:{DIGEST}"))?,
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn firecracker_executor_accepts_successful_roundtrip() -> Result<(), Box<dyn std::error::Error>>
    {
        let request = request()?;
        let mut executor = executor_with(Box::new(ScriptedSession::success()))?;
        let result = JudgeExecutor::execute(&mut executor, &request)?;
        assert_eq!(result.verdict(), Verdict::Accepted);
        assert_eq!(result.provenance().executor_kind().as_str(), "firecracker");
        executor.teardown();
        Ok(())
    }

    #[test]
    fn each_attempt_reclaims_vm_and_keeps_executor_reusable()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = request()?;
        let teardowns = Arc::new(AtomicUsize::new(0));
        let mut executor = FirecrackerExecutor::try_new(FirecrackerExecutorConfig {
            node_id: NodeId::parse("judge_fc_01")?,
            firecracker_path: "/usr/bin/firecracker".into(),
            api_socket: "/tmp/fc.sock".into(),
            vm_config: vm_config()?,
            workload: workload()?,
            production: false,
        })?;
        executor.activate_session(
            Box::new(ScriptedSession::success()),
            Box::new(RecordingVmLease {
                teardowns: Arc::clone(&teardowns),
            }),
        );

        let result = JudgeExecutor::execute(&mut executor, &request)?;
        assert_eq!(result.verdict(), Verdict::Accepted);
        assert_eq!(teardowns.load(Ordering::SeqCst), 1);

        executor.activate_session(
            Box::new(ScriptedSession::success()),
            Box::new(RecordingVmLease {
                teardowns: Arc::clone(&teardowns),
            }),
        );
        let next_result = JudgeExecutor::execute(&mut executor, &request)?;
        assert_eq!(next_result.verdict(), Verdict::Accepted);
        assert_eq!(teardowns.load(Ordering::SeqCst), 2);

        executor.teardown();
        executor.teardown();
        assert_eq!(teardowns.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bootstrap_failure_inside_worker_runtime_returns_error_without_panicking()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut executor = FirecrackerExecutor::try_new(FirecrackerExecutorConfig {
            node_id: NodeId::parse("judge_fc_01")?,
            firecracker_path: "/definitely/missing/firecracker".into(),
            api_socket: "/tmp/fc-missing.sock".into(),
            vm_config: vm_config()?,
            workload: workload()?,
            production: false,
        })?;

        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| executor.ensure_session()));

        assert!(
            matches!(outcome, Ok(Err(_))),
            "bootstrap failure must return an error without panicking the worker"
        );
        Ok(())
    }

    #[test]
    fn production_without_jailer_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let result = FirecrackerExecutorConfig {
            node_id: NodeId::parse("judge_fc_01")?,
            firecracker_path: "/usr/bin/firecracker".into(),
            api_socket: "/tmp/fc.sock".into(),
            vm_config: vm_config()?,
            workload: workload()?,
            production: true,
        }
        .validate();
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn production_with_jailer_remains_rejected_until_isolation_profile_is_complete()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = FirecrackerExecutorConfig {
            node_id: NodeId::parse("judge_fc_01")?,
            firecracker_path: "/usr/bin/firecracker".into(),
            api_socket: "/tmp/fc.sock".into(),
            vm_config: vm_config_with_jailer(Some("/usr/bin/jailer".into()))?,
            workload: workload()?,
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
