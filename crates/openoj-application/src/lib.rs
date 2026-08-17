mod control;

pub use control::{
    CancelEvaluation, ClaimTask, ControlPlane, CreateEvaluation, EvaluationSnapshot,
    EvaluationStore, LeasePolicy, NodePolicy, NodePolicyError, RetryExpired, StoreError,
    StoreFuture, SubmitResult, TaskLease,
};

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use openoj_domain::{
    Capability, Diagnostic, DomainError, EvaluationIdentity, EvaluationRequest, EvaluationResult,
    EvaluationResultParts, EvaluationStatus, EvidenceRef, ExecutionProvenance, ExecutorKind,
    NodeId, ResourceUsage, Score, StageKind, StageReport, StageStatus, Verdict,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationError {
    UnsupportedCapability,
    InvalidExecutorOutput { reason: &'static str },
    Domain(DomainError),
}

impl Display for ApplicationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCapability => {
                formatter.write_str("executor does not support a required capability")
            }
            Self::InvalidExecutorOutput { reason } => {
                write!(formatter, "executor returned an invalid outcome: {reason}")
            }
            Self::Domain(error) => {
                write!(formatter, "domain invariant rejected the result: {error}")
            }
        }
    }
}

impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Domain(error) => Some(error),
            Self::UnsupportedCapability | Self::InvalidExecutorOutput { .. } => None,
        }
    }
}

impl From<DomainError> for ApplicationError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decision {
    verdict: Verdict,
    score: Score,
}

impl Decision {
    /// Creates the deterministic decision produced by the successful check stage.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError`] when the verdict is not currently produced by a successful
    /// algorithm-batch checker.
    pub fn new(verdict: Verdict, score: Score) -> Result<Self, ApplicationError> {
        if !matches!(verdict, Verdict::Accepted | Verdict::WrongAnswer) {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "the check-stage decision must be accepted or wrong_answer",
            });
        }
        Ok(Self { verdict, score })
    }

    #[must_use]
    pub const fn verdict(self) -> Verdict {
        self.verdict
    }

    #[must_use]
    pub const fn score(self) -> Score {
        self.score
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageExecution {
    Succeeded {
        usage: ResourceUsage,
        diagnostics: Vec<Diagnostic>,
        evidence: Vec<EvidenceRef>,
        decision: Option<Decision>,
    },
    Rejected {
        verdict: Verdict,
        usage: ResourceUsage,
        diagnostics: Vec<Diagnostic>,
        evidence: Vec<EvidenceRef>,
    },
    Failed {
        usage: ResourceUsage,
        diagnostics: Vec<Diagnostic>,
        evidence: Vec<EvidenceRef>,
    },
    Cancelled {
        usage: ResourceUsage,
        diagnostics: Vec<Diagnostic>,
        evidence: Vec<EvidenceRef>,
    },
}

pub struct StageContext<'a> {
    request: &'a EvaluationRequest,
    stage: StageKind,
}

impl<'a> StageContext<'a> {
    #[must_use]
    pub const fn request(&self) -> &'a EvaluationRequest {
        self.request
    }

    #[must_use]
    pub const fn stage(&self) -> StageKind {
        self.stage
    }
}

pub trait StageExecutor {
    fn kind(&self) -> ExecutorKind;

    fn production_eligible(&self) -> bool;

    fn node_id(&self) -> Option<NodeId>;

    fn supports(&self, capability: &Capability) -> bool;

    fn execute(&mut self, context: StageContext<'_>) -> StageExecution;
}

/// Runs the canonical P0 stage plan through a bounded executor interface.
///
/// # Errors
///
/// Returns [`ApplicationError`] when capabilities are unsupported, executor output is
/// inconsistent, or a domain/result bound is violated.
pub fn evaluate<E: StageExecutor>(
    request: &EvaluationRequest,
    executor: &mut E,
) -> Result<EvaluationResult, ApplicationError> {
    if request
        .required_capabilities()
        .iter()
        .any(|capability| !executor.supports(capability))
    {
        return Err(ApplicationError::UnsupportedCapability);
    }

    let provenance = ExecutionProvenance::new(
        executor.kind(),
        executor.production_eligible(),
        request.runtime().digest().clone(),
        executor.node_id(),
    )?;
    let mut run = EvaluationRun::new(request, provenance);

    for (index, stage) in request.plan().stages().iter().copied().enumerate() {
        let outcome = executor.execute(StageContext { request, stage });
        if let Some(terminal) = run.record(index, stage, outcome)? {
            return run.finish(terminal);
        }
    }
    run.finish_success()
}

#[derive(Clone, Copy)]
struct Terminal {
    status: EvaluationStatus,
    verdict: Verdict,
    score: Score,
}

struct EvaluationRun<'a> {
    request: &'a EvaluationRequest,
    reports: Vec<StageReport>,
    usage: ResourceUsage,
    decision: Option<Decision>,
    provenance: ExecutionProvenance,
}

impl<'a> EvaluationRun<'a> {
    fn new(request: &'a EvaluationRequest, provenance: ExecutionProvenance) -> Self {
        Self {
            request,
            reports: Vec::with_capacity(request.plan().stages().len()),
            usage: ResourceUsage::default(),
            decision: None,
            provenance,
        }
    }

    fn record(
        &mut self,
        index: usize,
        stage: StageKind,
        outcome: StageExecution,
    ) -> Result<Option<Terminal>, ApplicationError> {
        let (status, usage, diagnostics, evidence, terminal, stage_decision) = match outcome {
            StageExecution::Succeeded {
                usage,
                diagnostics,
                evidence,
                decision,
            } => {
                validate_decision_location(stage, decision)?;
                (
                    StageStatus::Succeeded,
                    usage,
                    diagnostics,
                    evidence,
                    None,
                    decision,
                )
            }
            StageExecution::Rejected {
                verdict,
                usage,
                diagnostics,
                evidence,
            } => {
                validate_rejection(stage, verdict)?;
                let terminal = Terminal {
                    status: EvaluationStatus::Completed,
                    verdict,
                    score: zero_score()?,
                };
                (
                    StageStatus::Failed,
                    usage,
                    diagnostics,
                    evidence,
                    Some(terminal),
                    None,
                )
            }
            StageExecution::Failed {
                usage,
                diagnostics,
                evidence,
            } => (
                StageStatus::Failed,
                usage,
                diagnostics,
                evidence,
                Some(Terminal {
                    status: EvaluationStatus::Failed,
                    verdict: Verdict::SystemError,
                    score: zero_score()?,
                }),
                None,
            ),
            StageExecution::Cancelled {
                usage,
                diagnostics,
                evidence,
            } => (
                StageStatus::Cancelled,
                usage,
                diagnostics,
                evidence,
                Some(Terminal {
                    status: EvaluationStatus::Cancelled,
                    verdict: Verdict::Cancelled,
                    score: zero_score()?,
                }),
                None,
            ),
        };

        self.usage = self.usage.checked_add(usage)?;
        self.reports.push(StageReport::new(
            stage,
            status,
            usage,
            diagnostics,
            evidence,
        )?);
        if stage == StageKind::Check {
            self.decision = stage_decision;
        }
        if terminal.is_some() {
            append_skipped_reports(self.request, index + 1, &mut self.reports)?;
        }
        Ok(terminal)
    }

    fn finish_success(self) -> Result<EvaluationResult, ApplicationError> {
        let Some(decision) = self.decision else {
            return Err(ApplicationError::InvalidExecutorOutput {
                reason: "the check stage did not produce a decision",
            });
        };
        self.finish(Terminal {
            status: EvaluationStatus::Completed,
            verdict: decision.verdict(),
            score: decision.score(),
        })
    }

    fn finish(self, terminal: Terminal) -> Result<EvaluationResult, ApplicationError> {
        build_result(
            self.request,
            terminal.status,
            terminal.verdict,
            terminal.score,
            self.reports,
            self.usage,
            self.provenance,
        )
    }
}

fn validate_decision_location(
    stage: StageKind,
    decision: Option<Decision>,
) -> Result<(), ApplicationError> {
    if (stage == StageKind::Check) != decision.is_some() {
        return Err(ApplicationError::InvalidExecutorOutput {
            reason: "exactly the successful check stage must produce a decision",
        });
    }
    Ok(())
}

fn validate_rejection(stage: StageKind, verdict: Verdict) -> Result<(), ApplicationError> {
    let valid = match stage {
        StageKind::Build => verdict == Verdict::CompileError,
        StageKind::Run => matches!(
            verdict,
            Verdict::RuntimeError
                | Verdict::TimeLimitExceeded
                | Verdict::MemoryLimitExceeded
                | Verdict::OutputLimitExceeded
        ),
        StageKind::Prepare | StageKind::Check | StageKind::Aggregate => false,
    };
    if !valid {
        return Err(ApplicationError::InvalidExecutorOutput {
            reason: "stage rejection and verdict are inconsistent",
        });
    }
    Ok(())
}

fn append_skipped_reports(
    request: &EvaluationRequest,
    start: usize,
    reports: &mut Vec<StageReport>,
) -> Result<(), ApplicationError> {
    for stage in &request.plan().stages()[start..] {
        reports.push(StageReport::new(
            *stage,
            StageStatus::Skipped,
            ResourceUsage::default(),
            Vec::new(),
            Vec::new(),
        )?);
    }
    Ok(())
}

fn zero_score() -> Result<Score, ApplicationError> {
    Score::new(0, 1).map_err(ApplicationError::from)
}

fn build_result(
    request: &EvaluationRequest,
    status: EvaluationStatus,
    verdict: Verdict,
    score: Score,
    stages: Vec<StageReport>,
    usage: ResourceUsage,
    provenance: ExecutionProvenance,
) -> Result<EvaluationResult, ApplicationError> {
    EvaluationResult::new(EvaluationResultParts {
        identity: EvaluationIdentity::from_request(request),
        status,
        verdict,
        score,
        stages,
        usage,
        diagnostics: Vec::new(),
        evidence: Vec::new(),
        provenance,
    })
    .map_err(ApplicationError::from)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use openoj_domain::{
        ArtifactId, ArtifactRef, ArtifactSensitivity, AttemptId, Capability, ContentDigest,
        EvaluationId, EvaluationPlan, EvaluationRequest, EvaluationRequestParts, EvaluationStatus,
        ExecutorKind, IdempotencyKey, MediaType, NetworkPolicy, ProblemId, ProblemVersionId,
        ProblemVersionRef, RequestId, ResourcePolicy, ResourceUsage, RuntimeId, RuntimeRef, Score,
        StageKind, StageStatus, SubmissionId, SubmissionRef, Verdict,
    };

    use super::{Decision, StageContext, StageExecution, StageExecutor, evaluate};

    #[derive(Clone, Copy)]
    enum TerminalMode {
        CompileError,
        Cancelled,
        PlatformFailure,
    }

    struct TestExecutor {
        decision: Decision,
        terminal: Option<(StageKind, TerminalMode)>,
    }

    impl StageExecutor for TestExecutor {
        fn kind(&self) -> ExecutorKind {
            ExecutorKind::DevelopmentMock
        }

        fn production_eligible(&self) -> bool {
            false
        }

        fn node_id(&self) -> Option<openoj_domain::NodeId> {
            None
        }

        fn supports(&self, capability: &Capability) -> bool {
            capability.as_str() == "algorithm.batch"
        }

        fn execute(&mut self, context: StageContext<'_>) -> StageExecution {
            if let Some((stage, mode)) = self.terminal
                && stage == context.stage()
            {
                return match mode {
                    TerminalMode::CompileError => StageExecution::Rejected {
                        verdict: Verdict::CompileError,
                        usage: ResourceUsage::default(),
                        diagnostics: Vec::new(),
                        evidence: Vec::new(),
                    },
                    TerminalMode::Cancelled => StageExecution::Cancelled {
                        usage: ResourceUsage::default(),
                        diagnostics: Vec::new(),
                        evidence: Vec::new(),
                    },
                    TerminalMode::PlatformFailure => StageExecution::Failed {
                        usage: ResourceUsage::default(),
                        diagnostics: Vec::new(),
                        evidence: Vec::new(),
                    },
                };
            }
            StageExecution::Succeeded {
                usage: ResourceUsage::default(),
                diagnostics: Vec::new(),
                evidence: Vec::new(),
                decision: (context.stage() == StageKind::Check).then_some(self.decision),
            }
        }
    }

    #[test]
    fn mock_walks_the_canonical_pipeline() -> Result<(), Box<dyn Error>> {
        let request = request()?;
        let result = evaluate(&request, &mut executor(None)?)?;

        assert_eq!(result.verdict(), Verdict::Accepted);
        assert_eq!(result.stages().len(), 5);
        assert!(!result.provenance().production_eligible());
        Ok(())
    }

    #[test]
    fn unsupported_capability_is_rejected_before_execution() -> Result<(), Box<dyn Error>> {
        let mut request_parts = request_parts()?;
        request_parts.required_capabilities = vec![Capability::parse("network.egress")?];
        let request = EvaluationRequest::new(request_parts)?;

        assert!(evaluate(&request, &mut executor(None)?).is_err());
        Ok(())
    }

    #[test]
    fn compile_error_stops_build_and_skips_successors() -> Result<(), Box<dyn Error>> {
        let result = evaluate(
            &request()?,
            &mut executor(Some((StageKind::Build, TerminalMode::CompileError)))?,
        )?;

        assert_eq!(result.status(), EvaluationStatus::Completed);
        assert_eq!(result.verdict(), Verdict::CompileError);
        assert_eq!(result.stages()[1].status(), StageStatus::Failed);
        assert!(
            result.stages()[2..]
                .iter()
                .all(|stage| stage.status() == StageStatus::Skipped)
        );
        Ok(())
    }

    #[test]
    fn cancellation_and_platform_failure_have_distinct_results() -> Result<(), Box<dyn Error>> {
        let request = request()?;
        let cancelled = evaluate(
            &request,
            &mut executor(Some((StageKind::Run, TerminalMode::Cancelled)))?,
        )?;
        let failed = evaluate(
            &request,
            &mut executor(Some((StageKind::Prepare, TerminalMode::PlatformFailure)))?,
        )?;

        assert_eq!(cancelled.status(), EvaluationStatus::Cancelled);
        assert_eq!(cancelled.verdict(), Verdict::Cancelled);
        assert_eq!(failed.status(), EvaluationStatus::Failed);
        assert_eq!(failed.verdict(), Verdict::SystemError);
        Ok(())
    }

    fn executor(
        terminal: Option<(StageKind, TerminalMode)>,
    ) -> Result<TestExecutor, Box<dyn Error>> {
        Ok(TestExecutor {
            decision: Decision::new(Verdict::Accepted, Score::new(1, 1)?)?,
            terminal,
        })
    }

    fn request() -> Result<EvaluationRequest, Box<dyn Error>> {
        Ok(EvaluationRequest::new(request_parts()?)?)
    }

    fn request_parts() -> Result<EvaluationRequestParts, Box<dyn Error>> {
        let source = ArtifactRef::new(
            ArtifactId::parse("artifact_source_01")?,
            ContentDigest::parse(format!("sha256:{}", "2".repeat(64)))?,
            MediaType::parse("text/x-c++src")?,
            128,
            ArtifactSensitivity::Private,
        )?;
        Ok(EvaluationRequestParts {
            request_id: RequestId::parse("req_01")?,
            idempotency_key: IdempotencyKey::parse("idem_01")?,
            evaluation_id: EvaluationId::parse("eval_01")?,
            attempt_id: AttemptId::parse("attempt_01")?,
            attempt_number: 1,
            problem_version: ProblemVersionRef::new(
                ProblemId::parse("problem_01")?,
                ProblemVersionId::parse("problem_version_01")?,
                ContentDigest::parse(format!("sha256:{}", "1".repeat(64)))?,
            ),
            submission: SubmissionRef::new(SubmissionId::parse("submission_01")?, source),
            runtime: RuntimeRef::new(
                RuntimeId::parse("runtime_cpp_01")?,
                ContentDigest::parse(format!("sha256:{}", "3".repeat(64)))?,
            ),
            plan: EvaluationPlan::algorithm_batch(),
            policy: ResourcePolicy::new(
                1_000,
                2_000,
                268_435_456,
                1_048_576,
                64,
                NetworkPolicy::Denied,
            )?,
            required_capabilities: vec![Capability::parse("algorithm.batch")?],
        })
    }
}
