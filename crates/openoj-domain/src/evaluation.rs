use std::collections::BTreeSet;

use crate::{
    ArtifactRef, AttemptId, Capability, ContentDigest, DiagnosticCode, DomainError, EvaluationId,
    EvidenceId, EvidenceKind, IdempotencyKey, NodeId, ProblemId, ProblemVersionId, RequestId,
    RuntimeId, SubmissionId,
};

pub const MAX_CAPABILITIES: usize = 32;
pub const MAX_STAGES: usize = 32;
pub const MAX_STAGE_DIAGNOSTICS: usize = 64;
pub const MAX_STAGE_EVIDENCE: usize = 256;
pub const MAX_RESULT_DIAGNOSTICS: usize = 64;
pub const MAX_RESULT_EVIDENCE: usize = 256;
pub const MAX_DIAGNOSTIC_MESSAGE_LENGTH: usize = 2048;
pub const MAX_CPU_TIME_MS: u64 = 86_400_000;
pub const MAX_WALL_TIME_MS: u64 = 86_400_000;
pub const MAX_MEMORY_BYTES: u64 = 1_099_511_627_776;
pub const MAX_OUTPUT_BYTES: u64 = 1_073_741_824;
pub const MAX_PROCESS_LIMIT: u64 = 65_535;

const ALGORITHM_BATCH_STAGES: [StageKind; 5] = [
    StageKind::Prepare,
    StageKind::Build,
    StageKind::Run,
    StageKind::Check,
    StageKind::Aggregate,
];

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StageKind {
    Prepare,
    Build,
    Run,
    Check,
    Aggregate,
}

impl StageKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Build => "build",
            Self::Run => "run",
            Self::Check => "check",
            Self::Aggregate => "aggregate",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationProfile {
    AlgorithmBatch,
}

impl EvaluationProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlgorithmBatch => "algorithm_batch",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationPlan {
    profile: EvaluationProfile,
    stages: Vec<StageKind>,
}

impl EvaluationPlan {
    #[must_use]
    pub fn algorithm_batch() -> Self {
        Self {
            profile: EvaluationProfile::AlgorithmBatch,
            stages: ALGORITHM_BATCH_STAGES.to_vec(),
        }
    }

    /// Creates the current algorithm-batch plan from externally supplied stages.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] unless stages exactly match the canonical P0 order.
    pub fn try_algorithm_batch(stages: Vec<StageKind>) -> Result<Self, DomainError> {
        if stages.len() > MAX_STAGES {
            return Err(DomainError::TooManyItems {
                field: "plan.stages",
                maximum: MAX_STAGES,
                actual: stages.len(),
            });
        }
        if stages != ALGORITHM_BATCH_STAGES {
            return Err(DomainError::InvalidPlan);
        }
        Ok(Self {
            profile: EvaluationProfile::AlgorithmBatch,
            stages,
        })
    }

    #[must_use]
    pub const fn profile(&self) -> EvaluationProfile {
        self.profile
    }

    #[must_use]
    pub fn stages(&self) -> &[StageKind] {
        &self.stages
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkPolicy {
    Denied,
}

impl NetworkPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePolicy {
    cpu_time_ms: u64,
    wall_time_ms: u64,
    memory_bytes: u64,
    output_bytes: u64,
    process_limit: u32,
    network: NetworkPolicy,
}

impl ResourcePolicy {
    /// Creates a bounded execution policy whose network access is explicitly represented.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when any resource value is outside its protocol hard limit.
    pub fn new(
        cpu_time_ms: u64,
        wall_time_ms: u64,
        memory_bytes: u64,
        output_bytes: u64,
        process_limit: u32,
        network: NetworkPolicy,
    ) -> Result<Self, DomainError> {
        require_range("cpu_time_ms", cpu_time_ms, 1, MAX_CPU_TIME_MS)?;
        require_range("wall_time_ms", wall_time_ms, 1, MAX_WALL_TIME_MS)?;
        require_range("memory_bytes", memory_bytes, 1, MAX_MEMORY_BYTES)?;
        require_range("output_bytes", output_bytes, 1, MAX_OUTPUT_BYTES)?;
        require_range(
            "process_limit",
            u64::from(process_limit),
            1,
            MAX_PROCESS_LIMIT,
        )?;
        Ok(Self {
            cpu_time_ms,
            wall_time_ms,
            memory_bytes,
            output_bytes,
            process_limit,
            network,
        })
    }

    #[must_use]
    pub const fn cpu_time_ms(&self) -> u64 {
        self.cpu_time_ms
    }

    #[must_use]
    pub const fn wall_time_ms(&self) -> u64 {
        self.wall_time_ms
    }

    #[must_use]
    pub const fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }

    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }

    #[must_use]
    pub const fn process_limit(&self) -> u32 {
        self.process_limit
    }

    #[must_use]
    pub const fn network(&self) -> NetworkPolicy {
        self.network
    }
}

fn require_range(
    field: &'static str,
    actual: u64,
    minimum: u64,
    maximum: u64,
) -> Result<(), DomainError> {
    if !(minimum..=maximum).contains(&actual) {
        return Err(DomainError::OutOfRange {
            field,
            minimum,
            maximum,
            actual,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProblemVersionRef {
    problem_id: ProblemId,
    problem_version_id: ProblemVersionId,
    digest: ContentDigest,
}

impl ProblemVersionRef {
    #[must_use]
    pub const fn new(
        problem_id: ProblemId,
        problem_version_id: ProblemVersionId,
        digest: ContentDigest,
    ) -> Self {
        Self {
            problem_id,
            problem_version_id,
            digest,
        }
    }

    #[must_use]
    pub const fn problem_id(&self) -> &ProblemId {
        &self.problem_id
    }

    #[must_use]
    pub const fn problem_version_id(&self) -> &ProblemVersionId {
        &self.problem_version_id
    }

    #[must_use]
    pub const fn digest(&self) -> &ContentDigest {
        &self.digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRef {
    runtime_id: RuntimeId,
    digest: ContentDigest,
}

impl RuntimeRef {
    #[must_use]
    pub const fn new(runtime_id: RuntimeId, digest: ContentDigest) -> Self {
        Self { runtime_id, digest }
    }

    #[must_use]
    pub const fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }

    #[must_use]
    pub const fn digest(&self) -> &ContentDigest {
        &self.digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionRef {
    submission_id: SubmissionId,
    source: ArtifactRef,
}

impl SubmissionRef {
    #[must_use]
    pub const fn new(submission_id: SubmissionId, source: ArtifactRef) -> Self {
        Self {
            submission_id,
            source,
        }
    }

    #[must_use]
    pub const fn submission_id(&self) -> &SubmissionId {
        &self.submission_id
    }

    #[must_use]
    pub const fn source(&self) -> &ArtifactRef {
        &self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationRequest {
    request_id: RequestId,
    idempotency_key: IdempotencyKey,
    evaluation_id: EvaluationId,
    attempt_id: AttemptId,
    attempt_number: u32,
    problem_version: ProblemVersionRef,
    submission: SubmissionRef,
    runtime: RuntimeRef,
    plan: EvaluationPlan,
    policy: ResourcePolicy,
    required_capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationRequestParts {
    pub request_id: RequestId,
    pub idempotency_key: IdempotencyKey,
    pub evaluation_id: EvaluationId,
    pub attempt_id: AttemptId,
    pub attempt_number: u32,
    pub problem_version: ProblemVersionRef,
    pub submission: SubmissionRef,
    pub runtime: RuntimeRef,
    pub plan: EvaluationPlan,
    pub policy: ResourcePolicy,
    pub required_capabilities: Vec<Capability>,
}

impl EvaluationRequest {
    /// Creates a request after validating identity, capability, plan, and resource invariants.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] for an invalid attempt number or an oversized or duplicate
    /// capability set.
    pub fn new(parts: EvaluationRequestParts) -> Result<Self, DomainError> {
        require_range(
            "attempt_number",
            u64::from(parts.attempt_number),
            1,
            u64::from(u32::MAX),
        )?;
        if parts.required_capabilities.len() > MAX_CAPABILITIES {
            return Err(DomainError::TooManyItems {
                field: "required_capabilities",
                maximum: MAX_CAPABILITIES,
                actual: parts.required_capabilities.len(),
            });
        }
        let mut capabilities = BTreeSet::new();
        for capability in &parts.required_capabilities {
            if !capabilities.insert(capability) {
                return Err(DomainError::DuplicateItem {
                    field: "required_capabilities",
                });
            }
        }
        Ok(Self {
            request_id: parts.request_id,
            idempotency_key: parts.idempotency_key,
            evaluation_id: parts.evaluation_id,
            attempt_id: parts.attempt_id,
            attempt_number: parts.attempt_number,
            problem_version: parts.problem_version,
            submission: parts.submission,
            runtime: parts.runtime,
            plan: parts.plan,
            policy: parts.policy,
            required_capabilities: parts.required_capabilities,
        })
    }

    #[must_use]
    pub const fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    #[must_use]
    pub const fn idempotency_key(&self) -> &IdempotencyKey {
        &self.idempotency_key
    }

    #[must_use]
    pub const fn evaluation_id(&self) -> &EvaluationId {
        &self.evaluation_id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }

    #[must_use]
    pub const fn attempt_number(&self) -> u32 {
        self.attempt_number
    }

    #[must_use]
    pub const fn problem_version(&self) -> &ProblemVersionRef {
        &self.problem_version
    }

    #[must_use]
    pub const fn submission(&self) -> &SubmissionRef {
        &self.submission
    }

    #[must_use]
    pub const fn runtime(&self) -> &RuntimeRef {
        &self.runtime
    }

    #[must_use]
    pub const fn plan(&self) -> &EvaluationPlan {
        &self.plan
    }

    #[must_use]
    pub const fn policy(&self) -> &ResourcePolicy {
        &self.policy
    }

    #[must_use]
    pub fn required_capabilities(&self) -> &[Capability] {
        &self.required_capabilities
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceUsage {
    cpu_time_ms: u64,
    wall_time_ms: u64,
    memory_peak_bytes: u64,
    output_bytes: u64,
}

impl ResourceUsage {
    /// Creates a bounded resource-usage summary.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when a counter exceeds the protocol hard limit.
    pub fn new(
        cpu_time_ms: u64,
        wall_time_ms: u64,
        memory_peak_bytes: u64,
        output_bytes: u64,
    ) -> Result<Self, DomainError> {
        require_range("usage.cpu_time_ms", cpu_time_ms, 0, MAX_CPU_TIME_MS)?;
        require_range("usage.wall_time_ms", wall_time_ms, 0, MAX_WALL_TIME_MS)?;
        require_range(
            "usage.memory_peak_bytes",
            memory_peak_bytes,
            0,
            MAX_MEMORY_BYTES,
        )?;
        require_range("usage.output_bytes", output_bytes, 0, MAX_OUTPUT_BYTES)?;
        Ok(Self {
            cpu_time_ms,
            wall_time_ms,
            memory_peak_bytes,
            output_bytes,
        })
    }

    #[must_use]
    pub const fn cpu_time_ms(self) -> u64 {
        self.cpu_time_ms
    }

    #[must_use]
    pub const fn wall_time_ms(self) -> u64 {
        self.wall_time_ms
    }

    #[must_use]
    pub const fn memory_peak_bytes(self) -> u64 {
        self.memory_peak_bytes
    }

    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }

    /// Adds cumulative counters while taking the maximum memory peak.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] on integer overflow or when the aggregate exceeds a hard limit.
    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        let Some(cpu_time_ms) = self.cpu_time_ms.checked_add(other.cpu_time_ms) else {
            return Err(DomainError::OutOfRange {
                field: "usage.cpu_time_ms",
                minimum: 0,
                maximum: MAX_CPU_TIME_MS,
                actual: u64::MAX,
            });
        };
        let Some(wall_time_ms) = self.wall_time_ms.checked_add(other.wall_time_ms) else {
            return Err(DomainError::OutOfRange {
                field: "usage.wall_time_ms",
                minimum: 0,
                maximum: MAX_WALL_TIME_MS,
                actual: u64::MAX,
            });
        };
        let memory_peak_bytes = self.memory_peak_bytes.max(other.memory_peak_bytes);
        let Some(output_bytes) = self.output_bytes.checked_add(other.output_bytes) else {
            return Err(DomainError::OutOfRange {
                field: "usage.output_bytes",
                minimum: 0,
                maximum: MAX_OUTPUT_BYTES,
                actual: u64::MAX,
            });
        };
        Self::new(cpu_time_ms, wall_time_ms, memory_peak_bytes, output_bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: DiagnosticCode,
    message: String,
    truncated: bool,
}

impl Diagnostic {
    /// Creates a bounded, already-sanitized diagnostic.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the message exceeds the diagnostic character limit.
    pub fn new(
        code: DiagnosticCode,
        message: impl Into<String>,
        truncated: bool,
    ) -> Result<Self, DomainError> {
        let message = message.into();
        if message.chars().count() > MAX_DIAGNOSTIC_MESSAGE_LENGTH {
            return Err(DomainError::TooLong {
                field: "diagnostic.message",
                maximum: MAX_DIAGNOSTIC_MESSAGE_LENGTH,
                actual: message.chars().count(),
            });
        }
        Ok(Self {
            code,
            message,
            truncated,
        })
    }

    #[must_use]
    pub const fn code(&self) -> &DiagnosticCode {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRef {
    evidence_id: EvidenceId,
    kind: EvidenceKind,
    artifact: Option<ArtifactRef>,
}

impl EvidenceRef {
    #[must_use]
    pub const fn new(
        evidence_id: EvidenceId,
        kind: EvidenceKind,
        artifact: Option<ArtifactRef>,
    ) -> Self {
        Self {
            evidence_id,
            kind,
            artifact,
        }
    }

    #[must_use]
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }

    #[must_use]
    pub const fn kind(&self) -> &EvidenceKind {
        &self.kind
    }

    #[must_use]
    pub const fn artifact(&self) -> Option<&ArtifactRef> {
        self.artifact.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageStatus {
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

impl StageStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageReport {
    stage: StageKind,
    status: StageStatus,
    usage: ResourceUsage,
    diagnostics: Vec<Diagnostic>,
    evidence: Vec<EvidenceRef>,
}

impl StageReport {
    /// Creates a bounded report for one canonical stage.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when diagnostics or evidence exceed per-stage limits.
    pub fn new(
        stage: StageKind,
        status: StageStatus,
        usage: ResourceUsage,
        diagnostics: Vec<Diagnostic>,
        evidence: Vec<EvidenceRef>,
    ) -> Result<Self, DomainError> {
        enforce_collection_limit(
            "stage.diagnostics",
            diagnostics.len(),
            MAX_STAGE_DIAGNOSTICS,
        )?;
        enforce_collection_limit("stage.evidence", evidence.len(), MAX_STAGE_EVIDENCE)?;
        Ok(Self {
            stage,
            status,
            usage,
            diagnostics,
            evidence,
        })
    }

    #[must_use]
    pub const fn stage(&self) -> StageKind {
        self.stage
    }

    #[must_use]
    pub const fn status(&self) -> StageStatus {
        self.status
    }

    #[must_use]
    pub const fn usage(&self) -> ResourceUsage {
        self.usage
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn evidence(&self) -> &[EvidenceRef] {
        &self.evidence
    }
}

fn enforce_collection_limit(
    field: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), DomainError> {
    if actual > maximum {
        return Err(DomainError::TooManyItems {
            field,
            maximum,
            actual,
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationStatus {
    Completed,
    Failed,
    Cancelled,
}

impl EvaluationStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Accepted,
    WrongAnswer,
    CompileError,
    RuntimeError,
    TimeLimitExceeded,
    MemoryLimitExceeded,
    OutputLimitExceeded,
    SystemError,
    Cancelled,
}

impl Verdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::WrongAnswer => "wrong_answer",
            Self::CompileError => "compile_error",
            Self::RuntimeError => "runtime_error",
            Self::TimeLimitExceeded => "time_limit_exceeded",
            Self::MemoryLimitExceeded => "memory_limit_exceeded",
            Self::OutputLimitExceeded => "output_limit_exceeded",
            Self::SystemError => "system_error",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Score {
    earned: u32,
    possible: u32,
}

impl Score {
    /// Creates an integer score with a non-zero possible value.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when `possible` is zero or `earned` exceeds it.
    pub fn new(earned: u32, possible: u32) -> Result<Self, DomainError> {
        if possible == 0 || earned > possible {
            return Err(DomainError::InvalidScore);
        }
        Ok(Self { earned, possible })
    }

    #[must_use]
    pub const fn zero(possible: u32) -> Option<Self> {
        if possible == 0 {
            None
        } else {
            Some(Self {
                earned: 0,
                possible,
            })
        }
    }

    #[must_use]
    pub const fn earned(self) -> u32 {
        self.earned
    }

    #[must_use]
    pub const fn possible(self) -> u32 {
        self.possible
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorKind {
    DevelopmentMock,
    Firecracker,
}

impl ExecutorKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DevelopmentMock => "development_mock",
            Self::Firecracker => "firecracker",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionProvenance {
    executor_kind: ExecutorKind,
    production_eligible: bool,
    runtime_digest: ContentDigest,
    node_id: Option<NodeId>,
}

impl ExecutionProvenance {
    /// Creates execution provenance without allowing mock execution to appear production-safe.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] for production-eligible mock results or production results without
    /// a node identity.
    pub fn new(
        executor_kind: ExecutorKind,
        production_eligible: bool,
        runtime_digest: ContentDigest,
        node_id: Option<NodeId>,
    ) -> Result<Self, DomainError> {
        if executor_kind == ExecutorKind::DevelopmentMock && production_eligible {
            return Err(DomainError::InvalidProvenance {
                reason: "development mock results cannot be production eligible",
            });
        }
        if production_eligible && node_id.is_none() {
            return Err(DomainError::InvalidProvenance {
                reason: "production-eligible results require a node identity",
            });
        }
        Ok(Self {
            executor_kind,
            production_eligible,
            runtime_digest,
            node_id,
        })
    }

    #[must_use]
    pub fn development_mock(runtime_digest: ContentDigest) -> Self {
        Self {
            executor_kind: ExecutorKind::DevelopmentMock,
            production_eligible: false,
            runtime_digest,
            node_id: None,
        }
    }

    #[must_use]
    pub const fn executor_kind(&self) -> ExecutorKind {
        self.executor_kind
    }

    #[must_use]
    pub const fn production_eligible(&self) -> bool {
        self.production_eligible
    }

    #[must_use]
    pub const fn runtime_digest(&self) -> &ContentDigest {
        &self.runtime_digest
    }

    #[must_use]
    pub const fn node_id(&self) -> Option<&NodeId> {
        self.node_id.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationIdentity {
    request_id: RequestId,
    evaluation_id: EvaluationId,
    attempt_id: AttemptId,
    problem_version_id: ProblemVersionId,
    submission_id: SubmissionId,
    runtime_id: RuntimeId,
    runtime_digest: ContentDigest,
}

impl EvaluationIdentity {
    /// Creates the immutable identity carried by a canonical evaluation result.
    #[must_use]
    pub const fn new(
        request_id: RequestId,
        evaluation_id: EvaluationId,
        attempt_id: AttemptId,
        problem_version_id: ProblemVersionId,
        submission_id: SubmissionId,
        runtime_id: RuntimeId,
        runtime_digest: ContentDigest,
    ) -> Self {
        Self {
            request_id,
            evaluation_id,
            attempt_id,
            problem_version_id,
            submission_id,
            runtime_id,
            runtime_digest,
        }
    }

    #[must_use]
    pub fn from_request(request: &EvaluationRequest) -> Self {
        Self {
            request_id: request.request_id.clone(),
            evaluation_id: request.evaluation_id.clone(),
            attempt_id: request.attempt_id.clone(),
            problem_version_id: request.problem_version.problem_version_id.clone(),
            submission_id: request.submission.submission_id.clone(),
            runtime_id: request.runtime.runtime_id.clone(),
            runtime_digest: request.runtime.digest.clone(),
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> &RequestId {
        &self.request_id
    }

    #[must_use]
    pub const fn evaluation_id(&self) -> &EvaluationId {
        &self.evaluation_id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }

    #[must_use]
    pub const fn problem_version_id(&self) -> &ProblemVersionId {
        &self.problem_version_id
    }

    #[must_use]
    pub const fn submission_id(&self) -> &SubmissionId {
        &self.submission_id
    }

    #[must_use]
    pub const fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }

    #[must_use]
    pub const fn runtime_digest(&self) -> &ContentDigest {
        &self.runtime_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationResultParts {
    pub identity: EvaluationIdentity,
    pub status: EvaluationStatus,
    pub verdict: Verdict,
    pub score: Score,
    pub stages: Vec<StageReport>,
    pub usage: ResourceUsage,
    pub diagnostics: Vec<Diagnostic>,
    pub evidence: Vec<EvidenceRef>,
    pub provenance: ExecutionProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluationResult {
    identity: EvaluationIdentity,
    status: EvaluationStatus,
    verdict: Verdict,
    score: Score,
    stages: Vec<StageReport>,
    usage: ResourceUsage,
    diagnostics: Vec<Diagnostic>,
    evidence: Vec<EvidenceRef>,
    provenance: ExecutionProvenance,
}

impl EvaluationResult {
    /// Creates a result after checking stage order, terminal semantics, bounds, and provenance.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when any result invariant is inconsistent or unbounded.
    pub fn new(parts: EvaluationResultParts) -> Result<Self, DomainError> {
        if parts.stages.len() > MAX_STAGES || parts.stages.is_empty() {
            return Err(DomainError::InvalidResult {
                reason: "stage reports must be non-empty and bounded",
            });
        }
        let stage_kinds: Vec<_> = parts.stages.iter().map(StageReport::stage).collect();
        if stage_kinds != ALGORITHM_BATCH_STAGES {
            return Err(DomainError::InvalidResult {
                reason: "algorithm batch results must preserve the canonical stage order",
            });
        }
        enforce_collection_limit(
            "result.diagnostics",
            parts.diagnostics.len(),
            MAX_RESULT_DIAGNOSTICS,
        )?;
        enforce_collection_limit("result.evidence", parts.evidence.len(), MAX_RESULT_EVIDENCE)?;
        if parts.provenance.runtime_digest != parts.identity.runtime_digest {
            return Err(DomainError::InvalidProvenance {
                reason: "result provenance must match the requested runtime digest",
            });
        }
        validate_terminal_semantics(parts.status, parts.verdict, parts.score, &parts.stages)?;
        Ok(Self {
            identity: parts.identity,
            status: parts.status,
            verdict: parts.verdict,
            score: parts.score,
            stages: parts.stages,
            usage: parts.usage,
            diagnostics: parts.diagnostics,
            evidence: parts.evidence,
            provenance: parts.provenance,
        })
    }

    #[must_use]
    pub const fn identity(&self) -> &EvaluationIdentity {
        &self.identity
    }

    #[must_use]
    pub const fn status(&self) -> EvaluationStatus {
        self.status
    }

    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        self.verdict
    }

    #[must_use]
    pub const fn score(&self) -> Score {
        self.score
    }

    #[must_use]
    pub fn stages(&self) -> &[StageReport] {
        &self.stages
    }

    #[must_use]
    pub const fn usage(&self) -> ResourceUsage {
        self.usage
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn evidence(&self) -> &[EvidenceRef] {
        &self.evidence
    }

    #[must_use]
    pub const fn provenance(&self) -> &ExecutionProvenance {
        &self.provenance
    }
}

fn validate_terminal_semantics(
    status: EvaluationStatus,
    verdict: Verdict,
    score: Score,
    stages: &[StageReport],
) -> Result<(), DomainError> {
    let stage_statuses: Vec<_> = stages.iter().map(StageReport::status).collect();
    match status {
        EvaluationStatus::Completed => {
            if verdict == Verdict::Cancelled || verdict == Verdict::SystemError {
                return Err(DomainError::InvalidResult {
                    reason: "completed results cannot use cancelled or system-error verdicts",
                });
            }
            match verdict {
                Verdict::Accepted | Verdict::WrongAnswer => {
                    if stage_statuses
                        .iter()
                        .any(|stage| *stage != StageStatus::Succeeded)
                    {
                        return Err(DomainError::InvalidResult {
                            reason: "accepted and wrong-answer results require all stages to succeed",
                        });
                    }
                }
                Verdict::CompileError => {
                    validate_rejected_stage(stages, StageKind::Build)?;
                    if score.earned() != 0 {
                        return Err(DomainError::InvalidResult {
                            reason: "compile-error results require a zero score",
                        });
                    }
                }
                Verdict::RuntimeError
                | Verdict::TimeLimitExceeded
                | Verdict::MemoryLimitExceeded
                | Verdict::OutputLimitExceeded => {
                    validate_rejected_stage(stages, StageKind::Run)?;
                    if score.earned() != 0 {
                        return Err(DomainError::InvalidResult {
                            reason: "run-rejected results require a zero score",
                        });
                    }
                }
                Verdict::SystemError | Verdict::Cancelled => {
                    return Err(DomainError::InvalidResult {
                        reason: "completed results use an invalid terminal verdict",
                    });
                }
            }
        }
        EvaluationStatus::Failed => {
            if verdict != Verdict::SystemError || score.earned() != 0 {
                return Err(DomainError::InvalidResult {
                    reason: "failed results require system_error and a zero score",
                });
            }
            validate_terminal_stage_pattern(stages, StageStatus::Failed)?;
        }
        EvaluationStatus::Cancelled => {
            if verdict != Verdict::Cancelled || score.earned() != 0 {
                return Err(DomainError::InvalidResult {
                    reason: "cancelled results require cancelled verdict and a zero score",
                });
            }
            validate_terminal_stage_pattern(stages, StageStatus::Cancelled)?;
        }
    }
    Ok(())
}

fn validate_terminal_stage_pattern(
    stages: &[StageReport],
    terminal_status: StageStatus,
) -> Result<(), DomainError> {
    let Some(terminal_index) = stages
        .iter()
        .position(|stage| stage.status() == terminal_status)
    else {
        return Err(DomainError::InvalidResult {
            reason: "terminal result does not contain its terminal stage status",
        });
    };
    if stages[..terminal_index]
        .iter()
        .any(|stage| stage.status() != StageStatus::Succeeded)
        || stages[terminal_index + 1..]
            .iter()
            .any(|stage| stage.status() != StageStatus::Skipped)
    {
        return Err(DomainError::InvalidResult {
            reason: "terminal stage requires succeeded predecessors and skipped successors",
        });
    }
    Ok(())
}

fn validate_rejected_stage(
    stages: &[StageReport],
    rejected_stage: StageKind,
) -> Result<(), DomainError> {
    let Some(rejected_index) = stages
        .iter()
        .position(|stage| stage.stage() == rejected_stage)
    else {
        return Err(DomainError::InvalidResult {
            reason: "the verdict does not map to a canonical stage",
        });
    };
    if stages[rejected_index].status() != StageStatus::Failed
        || stages[..rejected_index]
            .iter()
            .any(|stage| stage.status() != StageStatus::Succeeded)
        || stages[rejected_index + 1..]
            .iter()
            .any(|stage| stage.status() != StageStatus::Skipped)
    {
        return Err(DomainError::InvalidResult {
            reason: "rejected results require succeeded predecessors and skipped successors",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        EvaluationPlan, MAX_OUTPUT_BYTES, NetworkPolicy, ResourcePolicy, Score, StageKind,
    };

    #[test]
    fn algorithm_plan_rejects_reordering_and_omission() {
        assert_eq!(EvaluationPlan::algorithm_batch().stages().len(), 5);
        assert!(
            EvaluationPlan::try_algorithm_batch(vec![StageKind::Prepare, StageKind::Run]).is_err()
        );
    }

    #[test]
    fn resource_policy_is_bounded_and_network_is_denied() {
        assert!(ResourcePolicy::new(1, 1, 1, MAX_OUTPUT_BYTES, 1, NetworkPolicy::Denied).is_ok());
        assert!(
            ResourcePolicy::new(1, 1, 1, MAX_OUTPUT_BYTES + 1, 1, NetworkPolicy::Denied).is_err()
        );
    }

    #[test]
    fn score_cannot_exceed_possible_points() {
        assert!(Score::new(1, 1).is_ok());
        assert!(Score::new(2, 1).is_err());
        assert!(Score::new(0, 0).is_err());
    }
}
