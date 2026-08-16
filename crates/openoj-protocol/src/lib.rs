use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::OnceLock;

use openoj_domain::{
    ArtifactId, ArtifactRef, ArtifactSensitivity, AttemptId, Capability, ContentDigest, Diagnostic,
    DomainError, EvaluationId, EvaluationPlan, EvaluationRequest, EvaluationRequestParts,
    EvaluationResult, EvidenceRef, IdempotencyKey, MediaType, NetworkPolicy, ProblemId,
    ProblemVersionId, ProblemVersionRef, RequestId, ResourcePolicy, ResourceUsage, RuntimeId,
    RuntimeRef, StageKind, SubmissionId, SubmissionRef,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

pub const SCHEMA_VERSION: &str = "openoj.evaluation/v0alpha1";
pub const MAX_EVALUATION_REQUEST_BYTES: usize = 262_144;
pub const MAX_EVALUATION_RESULT_BYTES: usize = 1_048_576;

const SCHEMA_JSON: &str =
    include_str!("../../../schemas/openoj/v0alpha1/open-evaluation.schema.json");

static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

pub mod wire {
    typify::import_types!(schema = "../../schemas/openoj/v0alpha1/open-evaluation.schema.json");
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProtocolError {
    MessageTooLarge { maximum: usize, actual: usize },
    MalformedDocument,
    SchemaUnavailable,
    SchemaViolation,
    SemanticViolation,
    Domain(DomainError),
    EncodingFailed,
}

impl Display for ProtocolError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageTooLarge { maximum, actual } => {
                write!(
                    formatter,
                    "message exceeds its byte limit ({actual} > {maximum})"
                )
            }
            Self::MalformedDocument => formatter.write_str("message is not valid typed JSON"),
            Self::SchemaUnavailable => {
                formatter.write_str("canonical schema could not be initialized")
            }
            Self::SchemaViolation => {
                formatter.write_str("message does not satisfy the canonical schema")
            }
            Self::SemanticViolation => {
                formatter.write_str("message violates canonical cross-field semantics")
            }
            Self::Domain(error) => {
                write!(formatter, "message violates a domain invariant: {error}")
            }
            Self::EncodingFailed => formatter.write_str("message could not be encoded"),
        }
    }
}

impl Error for ProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Domain(error) => Some(error),
            Self::MessageTooLarge { .. }
            | Self::MalformedDocument
            | Self::SchemaUnavailable
            | Self::SchemaViolation
            | Self::SemanticViolation
            | Self::EncodingFailed => None,
        }
    }
}

impl From<DomainError> for ProtocolError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

/// Decodes and validates a bounded canonical evaluation request.
///
/// # Errors
///
/// Returns [`ProtocolError`] for oversized, malformed, schema-invalid, or domain-invalid input.
pub fn decode_evaluation_request(input: &[u8]) -> Result<EvaluationRequest, ProtocolError> {
    let value = decode_wire::<wire::EvaluationRequest>(input, MAX_EVALUATION_REQUEST_BYTES)?;
    request_from_value(&value)
}

/// Encodes a validated domain request through the generated canonical wire type.
///
/// # Errors
///
/// Returns [`ProtocolError`] when schema validation or bounded encoding fails.
pub fn encode_evaluation_request(request: &EvaluationRequest) -> Result<Vec<u8>, ProtocolError> {
    encode_wire::<wire::EvaluationRequest>(request_to_value(request), MAX_EVALUATION_REQUEST_BYTES)
}

/// Decodes a bounded canonical result into its schema-generated wire type.
///
/// # Errors
///
/// Returns [`ProtocolError`] for oversized, malformed, or schema-invalid input.
pub fn decode_evaluation_result(input: &[u8]) -> Result<wire::EvaluationResult, ProtocolError> {
    ensure_size(input.len(), MAX_EVALUATION_RESULT_BYTES)?;
    let result: wire::EvaluationResult =
        serde_json::from_slice(input).map_err(|_| ProtocolError::MalformedDocument)?;
    let value = serde_json::to_value(&result).map_err(|_| ProtocolError::EncodingFailed)?;
    validate(&value)?;
    validate_result_semantics(&value)?;
    Ok(result)
}

/// Encodes a validated domain result through the generated canonical wire type.
///
/// # Errors
///
/// Returns [`ProtocolError`] when schema validation or bounded encoding fails.
pub fn encode_evaluation_result(result: &EvaluationResult) -> Result<Vec<u8>, ProtocolError> {
    let value = result_to_value(result);
    validate_result_semantics(&value)?;
    encode_wire::<wire::EvaluationResult>(value, MAX_EVALUATION_RESULT_BYTES)
}

fn decode_wire<T>(input: &[u8], maximum: usize) -> Result<Value, ProtocolError>
where
    T: DeserializeOwned + Serialize,
{
    ensure_size(input.len(), maximum)?;
    let typed: T = serde_json::from_slice(input).map_err(|_| ProtocolError::MalformedDocument)?;
    let value = serde_json::to_value(typed).map_err(|_| ProtocolError::EncodingFailed)?;
    validate(&value)?;
    Ok(value)
}

fn encode_wire<T>(value: Value, maximum: usize) -> Result<Vec<u8>, ProtocolError>
where
    T: DeserializeOwned + Serialize,
{
    validate(&value)?;
    let typed: T = serde_json::from_value(value).map_err(|_| ProtocolError::SchemaViolation)?;
    let encoded = serde_json::to_vec(&typed).map_err(|_| ProtocolError::EncodingFailed)?;
    ensure_size(encoded.len(), maximum)?;
    Ok(encoded)
}

fn ensure_size(actual: usize, maximum: usize) -> Result<(), ProtocolError> {
    if actual > maximum {
        return Err(ProtocolError::MessageTooLarge { maximum, actual });
    }
    Ok(())
}

fn validate(value: &Value) -> Result<(), ProtocolError> {
    let validator = validator()?;
    if validator.is_valid(value) {
        Ok(())
    } else {
        Err(ProtocolError::SchemaViolation)
    }
}

fn validator() -> Result<&'static jsonschema::Validator, ProtocolError> {
    if let Some(validator) = VALIDATOR.get() {
        return Ok(validator);
    }

    let schema: Value =
        serde_json::from_str(SCHEMA_JSON).map_err(|_| ProtocolError::SchemaUnavailable)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|_| ProtocolError::SchemaUnavailable)?;
    let _already_initialized = VALIDATOR.set(validator);
    VALIDATOR.get().ok_or(ProtocolError::SchemaUnavailable)
}

fn validate_result_semantics(value: &Value) -> Result<(), ProtocolError> {
    let result = as_object(value)?;
    let status = string_field(result, "status")?;
    let verdict = string_field(result, "verdict")?;
    let score = as_object(field(result, "score")?)?;
    let earned = u64_field(score, "earned")?;
    let possible = u64_field(score, "possible")?;
    let stages = as_array(field(result, "stages")?)?;
    let provenance = as_object(field(result, "provenance")?)?;

    if earned > possible {
        return Err(ProtocolError::SemanticViolation);
    }
    match status {
        "completed" if matches!(verdict, "cancelled" | "system_error") => {
            return Err(ProtocolError::SemanticViolation);
        }
        "failed" if verdict != "system_error" || earned != 0 => {
            return Err(ProtocolError::SemanticViolation);
        }
        "cancelled" if verdict != "cancelled" || earned != 0 => {
            return Err(ProtocolError::SemanticViolation);
        }
        "completed" | "failed" | "cancelled" => {}
        _ => return Err(ProtocolError::SemanticViolation),
    }
    if status == "completed"
        && matches!(
            verdict,
            "compile_error"
                | "runtime_error"
                | "time_limit_exceeded"
                | "memory_limit_exceeded"
                | "output_limit_exceeded"
        )
        && earned != 0
    {
        return Err(ProtocolError::SemanticViolation);
    }

    validate_result_stages(stages, status, verdict)?;
    validate_aggregate_usage(result, stages)?;
    let executor_kind = string_field(provenance, "executor_kind")?;
    let production_eligible = field(provenance, "production_eligible")?
        .as_bool()
        .ok_or(ProtocolError::SemanticViolation)?;
    if production_eligible
        && (executor_kind == "development_mock" || !provenance.contains_key("node_id"))
    {
        return Err(ProtocolError::SemanticViolation);
    }
    Ok(())
}

fn validate_aggregate_usage(
    result: &Map<String, Value>,
    stages: &[Value],
) -> Result<(), ProtocolError> {
    let mut cpu_time_ms = 0_u64;
    let mut wall_time_ms = 0_u64;
    let mut memory_peak_bytes = 0_u64;
    let mut output_bytes = 0_u64;
    for stage in stages {
        let report = as_object(stage)?;
        let usage = as_object(field(report, "usage")?)?;
        cpu_time_ms = cpu_time_ms
            .checked_add(u64_field(usage, "cpu_time_ms")?)
            .ok_or(ProtocolError::SemanticViolation)?;
        wall_time_ms = wall_time_ms
            .checked_add(u64_field(usage, "wall_time_ms")?)
            .ok_or(ProtocolError::SemanticViolation)?;
        memory_peak_bytes = memory_peak_bytes.max(u64_field(usage, "memory_peak_bytes")?);
        output_bytes = output_bytes
            .checked_add(u64_field(usage, "output_bytes")?)
            .ok_or(ProtocolError::SemanticViolation)?;
    }

    let aggregate = as_object(field(result, "usage")?)?;
    if u64_field(aggregate, "cpu_time_ms")? != cpu_time_ms
        || u64_field(aggregate, "wall_time_ms")? != wall_time_ms
        || u64_field(aggregate, "memory_peak_bytes")? != memory_peak_bytes
        || u64_field(aggregate, "output_bytes")? != output_bytes
    {
        return Err(ProtocolError::SemanticViolation);
    }
    Ok(())
}

fn validate_result_stages(
    stages: &[Value],
    status: &str,
    verdict: &str,
) -> Result<(), ProtocolError> {
    const EXPECTED: [&str; 5] = ["prepare", "build", "run", "check", "aggregate"];
    if stages.len() != EXPECTED.len() {
        return Err(ProtocolError::SemanticViolation);
    }
    let mut statuses = Vec::with_capacity(stages.len());
    for (stage, expected) in stages.iter().zip(EXPECTED) {
        let report = as_object(stage)?;
        if string_field(report, "stage")? != expected {
            return Err(ProtocolError::SemanticViolation);
        }
        statuses.push(string_field(report, "status")?);
    }

    if status == "completed" && matches!(verdict, "accepted" | "wrong_answer") {
        if statuses.iter().any(|stage| *stage != "succeeded") {
            return Err(ProtocolError::SemanticViolation);
        }
        return Ok(());
    }

    let expected_terminal = match (status, verdict) {
        ("completed", "compile_error") => (1, "failed"),
        (
            "completed",
            "runtime_error"
            | "time_limit_exceeded"
            | "memory_limit_exceeded"
            | "output_limit_exceeded",
        ) => (2, "failed"),
        ("failed", "system_error") => {
            return validate_any_terminal_pattern(&statuses, "failed");
        }
        ("cancelled", "cancelled") => {
            return validate_any_terminal_pattern(&statuses, "cancelled");
        }
        _ => return Err(ProtocolError::SemanticViolation),
    };
    validate_terminal_pattern(&statuses, expected_terminal.0, expected_terminal.1)
}

fn validate_any_terminal_pattern(
    statuses: &[&str],
    terminal_status: &str,
) -> Result<(), ProtocolError> {
    let Some(index) = statuses
        .iter()
        .position(|status| *status == terminal_status)
    else {
        return Err(ProtocolError::SemanticViolation);
    };
    validate_terminal_pattern(statuses, index, terminal_status)
}

fn validate_terminal_pattern(
    statuses: &[&str],
    terminal_index: usize,
    terminal_status: &str,
) -> Result<(), ProtocolError> {
    if statuses.get(terminal_index) != Some(&terminal_status)
        || statuses[..terminal_index]
            .iter()
            .any(|status| *status != "succeeded")
        || statuses[terminal_index + 1..]
            .iter()
            .any(|status| *status != "skipped")
    {
        return Err(ProtocolError::SemanticViolation);
    }
    Ok(())
}

fn request_from_value(value: &Value) -> Result<EvaluationRequest, ProtocolError> {
    let object = as_object(value)?;
    let problem = as_object(field(object, "problem_version")?)?;
    let submission = as_object(field(object, "submission")?)?;
    let source = artifact_from_value(field(submission, "source")?)?;
    let runtime = as_object(field(object, "runtime")?)?;
    let plan = as_object(field(object, "plan")?)?;
    let policy = as_object(field(object, "policy")?)?;

    let stages = as_array(field(plan, "stages")?)?
        .iter()
        .map(stage_kind_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    if string_field(plan, "profile")? != "algorithm_batch" {
        return Err(ProtocolError::SchemaViolation);
    }

    let required_capabilities = as_array(field(object, "required_capabilities")?)?
        .iter()
        .map(|value| Capability::parse(as_str(value)?).map_err(ProtocolError::from))
        .collect::<Result<Vec<_>, _>>()?;
    let attempt_number = u32::try_from(u64_field(object, "attempt_number")?)
        .map_err(|_| ProtocolError::SchemaViolation)?;
    let process_limit = u32::try_from(u64_field(policy, "process_limit")?)
        .map_err(|_| ProtocolError::SchemaViolation)?;
    if string_field(policy, "network")? != "denied" {
        return Err(ProtocolError::SchemaViolation);
    }

    EvaluationRequest::new(EvaluationRequestParts {
        request_id: RequestId::parse(string_field(object, "request_id")?)?,
        idempotency_key: IdempotencyKey::parse(string_field(object, "idempotency_key")?)?,
        evaluation_id: EvaluationId::parse(string_field(object, "evaluation_id")?)?,
        attempt_id: AttemptId::parse(string_field(object, "attempt_id")?)?,
        attempt_number,
        problem_version: ProblemVersionRef::new(
            ProblemId::parse(string_field(problem, "problem_id")?)?,
            ProblemVersionId::parse(string_field(problem, "problem_version_id")?)?,
            ContentDigest::parse(string_field(problem, "digest")?)?,
        ),
        submission: SubmissionRef::new(
            SubmissionId::parse(string_field(submission, "submission_id")?)?,
            source,
        ),
        runtime: RuntimeRef::new(
            RuntimeId::parse(string_field(runtime, "runtime_id")?)?,
            ContentDigest::parse(string_field(runtime, "digest")?)?,
        ),
        plan: EvaluationPlan::try_algorithm_batch(stages)?,
        policy: ResourcePolicy::new(
            u64_field(policy, "cpu_time_ms")?,
            u64_field(policy, "wall_time_ms")?,
            u64_field(policy, "memory_bytes")?,
            u64_field(policy, "output_bytes")?,
            process_limit,
            NetworkPolicy::Denied,
        )?,
        required_capabilities,
    })
    .map_err(ProtocolError::from)
}

fn artifact_from_value(value: &Value) -> Result<ArtifactRef, ProtocolError> {
    let artifact = as_object(value)?;
    let sensitivity = match string_field(artifact, "sensitivity")? {
        "public" => ArtifactSensitivity::Public,
        "private" => ArtifactSensitivity::Private,
        "hidden" => ArtifactSensitivity::Hidden,
        _ => return Err(ProtocolError::SchemaViolation),
    };
    ArtifactRef::new(
        ArtifactId::parse(string_field(artifact, "artifact_id")?)?,
        ContentDigest::parse(string_field(artifact, "digest")?)?,
        MediaType::parse(string_field(artifact, "media_type")?)?,
        u64_field(artifact, "size_bytes")?,
        sensitivity,
    )
    .map_err(ProtocolError::from)
}

fn stage_kind_from_value(value: &Value) -> Result<StageKind, ProtocolError> {
    match as_str(value)? {
        "prepare" => Ok(StageKind::Prepare),
        "build" => Ok(StageKind::Build),
        "run" => Ok(StageKind::Run),
        "check" => Ok(StageKind::Check),
        "aggregate" => Ok(StageKind::Aggregate),
        _ => Err(ProtocolError::SchemaViolation),
    }
}

fn request_to_value(request: &EvaluationRequest) -> Value {
    json!({
        "message_type": "evaluation_request",
        "schema_version": SCHEMA_VERSION,
        "request_id": request.request_id().as_str(),
        "idempotency_key": request.idempotency_key().as_str(),
        "evaluation_id": request.evaluation_id().as_str(),
        "attempt_id": request.attempt_id().as_str(),
        "attempt_number": request.attempt_number(),
        "problem_version": {
            "problem_id": request.problem_version().problem_id().as_str(),
            "problem_version_id": request.problem_version().problem_version_id().as_str(),
            "digest": request.problem_version().digest().as_str()
        },
        "submission": {
            "submission_id": request.submission().submission_id().as_str(),
            "source": artifact_to_value(request.submission().source())
        },
        "runtime": {
            "runtime_id": request.runtime().runtime_id().as_str(),
            "digest": request.runtime().digest().as_str()
        },
        "plan": {
            "profile": request.plan().profile().as_str(),
            "stages": request
                .plan()
                .stages()
                .iter()
                .map(|stage| stage.as_str())
                .collect::<Vec<_>>()
        },
        "policy": {
            "cpu_time_ms": request.policy().cpu_time_ms(),
            "wall_time_ms": request.policy().wall_time_ms(),
            "memory_bytes": request.policy().memory_bytes(),
            "output_bytes": request.policy().output_bytes(),
            "process_limit": request.policy().process_limit(),
            "network": request.policy().network().as_str()
        },
        "required_capabilities": request
            .required_capabilities()
            .iter()
            .map(Capability::as_str)
            .collect::<Vec<_>>()
    })
}

fn result_to_value(result: &EvaluationResult) -> Value {
    json!({
        "message_type": "evaluation_result",
        "schema_version": SCHEMA_VERSION,
        "request_id": result.identity().request_id().as_str(),
        "evaluation_id": result.identity().evaluation_id().as_str(),
        "attempt_id": result.identity().attempt_id().as_str(),
        "problem_version_id": result.identity().problem_version_id().as_str(),
        "submission_id": result.identity().submission_id().as_str(),
        "runtime_id": result.identity().runtime_id().as_str(),
        "status": result.status().as_str(),
        "verdict": result.verdict().as_str(),
        "score": score_to_value(result.score()),
        "stages": result.stages().iter().map(stage_report_to_value).collect::<Vec<_>>(),
        "usage": usage_to_value(result.usage()),
        "diagnostics": result.diagnostics().iter().map(diagnostic_to_value).collect::<Vec<_>>(),
        "evidence": result.evidence().iter().map(evidence_to_value).collect::<Vec<_>>(),
        "provenance": provenance_to_value(result.provenance())
    })
}

fn provenance_to_value(provenance: &openoj_domain::ExecutionProvenance) -> Value {
    let mut value = json!({
        "executor_kind": provenance.executor_kind().as_str(),
        "production_eligible": provenance.production_eligible(),
        "runtime_digest": provenance.runtime_digest().as_str()
    });
    if let (Some(object), Some(node_id)) = (value.as_object_mut(), provenance.node_id()) {
        object.insert("node_id".to_owned(), json!(node_id.as_str()));
    }
    value
}

fn artifact_to_value(artifact: &ArtifactRef) -> Value {
    json!({
        "artifact_id": artifact.artifact_id().as_str(),
        "digest": artifact.digest().as_str(),
        "media_type": artifact.media_type().as_str(),
        "size_bytes": artifact.size_bytes(),
        "sensitivity": match artifact.sensitivity() {
            ArtifactSensitivity::Public => "public",
            ArtifactSensitivity::Private => "private",
            ArtifactSensitivity::Hidden => "hidden"
        }
    })
}

fn score_to_value(score: openoj_domain::Score) -> Value {
    json!({
        "earned": score.earned(),
        "possible": score.possible()
    })
}

fn usage_to_value(usage: ResourceUsage) -> Value {
    json!({
        "cpu_time_ms": usage.cpu_time_ms(),
        "wall_time_ms": usage.wall_time_ms(),
        "memory_peak_bytes": usage.memory_peak_bytes(),
        "output_bytes": usage.output_bytes()
    })
}

fn stage_report_to_value(report: &openoj_domain::StageReport) -> Value {
    json!({
        "stage": report.stage().as_str(),
        "status": report.status().as_str(),
        "usage": usage_to_value(report.usage()),
        "diagnostics": report.diagnostics().iter().map(diagnostic_to_value).collect::<Vec<_>>(),
        "evidence": report.evidence().iter().map(evidence_to_value).collect::<Vec<_>>()
    })
}

fn diagnostic_to_value(diagnostic: &Diagnostic) -> Value {
    json!({
        "code": diagnostic.code().as_str(),
        "message": diagnostic.message(),
        "truncated": diagnostic.truncated()
    })
}

fn evidence_to_value(evidence: &EvidenceRef) -> Value {
    let mut value = json!({
        "evidence_id": evidence.evidence_id().as_str(),
        "kind": evidence.kind().as_str()
    });
    if let (Some(object), Some(artifact)) = (value.as_object_mut(), evidence.artifact()) {
        object.insert("artifact".to_owned(), artifact_to_value(artifact));
    }
    value
}

fn as_object(value: &Value) -> Result<&Map<String, Value>, ProtocolError> {
    value.as_object().ok_or(ProtocolError::SchemaViolation)
}

fn as_array(value: &Value) -> Result<&[Value], ProtocolError> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(ProtocolError::SchemaViolation)
}

fn as_str(value: &Value) -> Result<&str, ProtocolError> {
    value.as_str().ok_or(ProtocolError::SchemaViolation)
}

fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a Value, ProtocolError> {
    object.get(name).ok_or(ProtocolError::SchemaViolation)
}

fn string_field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, ProtocolError> {
    as_str(field(object, name)?)
}

fn u64_field(object: &Map<String, Value>, name: &str) -> Result<u64, ProtocolError> {
    field(object, name)?
        .as_u64()
        .ok_or(ProtocolError::SchemaViolation)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde_json::{Value, json};

    use super::{
        MAX_EVALUATION_REQUEST_BYTES, ProtocolError, SCHEMA_JSON, decode_evaluation_request,
        decode_evaluation_result, encode_evaluation_request,
    };

    const VALID_REQUEST: &[u8] =
        include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");
    const VALID_RESULT: &[u8] =
        include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-result.valid.json");

    #[test]
    fn canonical_schema_is_valid_draft_2020_12() -> Result<(), Box<dyn Error>> {
        let schema: Value = serde_json::from_str(SCHEMA_JSON)?;
        assert!(jsonschema::meta::is_valid(&schema));
        Ok(())
    }

    #[test]
    fn valid_request_round_trips_through_domain() -> Result<(), Box<dyn Error>> {
        let request = decode_evaluation_request(VALID_REQUEST)?;
        let encoded = encode_evaluation_request(&request)?;
        let decoded = decode_evaluation_request(&encoded)?;

        assert_eq!(request, decoded);
        Ok(())
    }

    #[test]
    fn unknown_field_and_profile_fail_closed() -> Result<(), Box<dyn Error>> {
        let mut unknown_field: Value = serde_json::from_slice(VALID_REQUEST)?;
        if let Some(object) = unknown_field.as_object_mut() {
            object.insert("unexpected".to_owned(), json!(true));
        }
        assert!(matches!(
            decode_evaluation_request(&serde_json::to_vec(&unknown_field)?),
            Err(ProtocolError::MalformedDocument | ProtocolError::SchemaViolation)
        ));

        let mut unknown_profile: Value = serde_json::from_slice(VALID_REQUEST)?;
        unknown_profile["plan"]["profile"] = json!("engineering_project");
        assert!(decode_evaluation_request(&serde_json::to_vec(&unknown_profile)?).is_err());

        let mut unknown_version: Value = serde_json::from_slice(VALID_REQUEST)?;
        unknown_version["schema_version"] = json!("openoj.evaluation/v0alpha2");
        assert!(decode_evaluation_request(&serde_json::to_vec(&unknown_version)?).is_err());
        Ok(())
    }

    #[test]
    fn duplicate_fields_and_oversized_messages_are_rejected() -> Result<(), Box<dyn Error>> {
        let valid = std::str::from_utf8(VALID_REQUEST)?;
        let duplicate = valid.replacen(
            "\"message_type\": \"evaluation_request\",",
            "\"message_type\": \"evaluation_request\",\n  \"message_type\": \"evaluation_request\",",
            1,
        );
        assert!(decode_evaluation_request(duplicate.as_bytes()).is_err());

        let oversized = vec![b' '; MAX_EVALUATION_REQUEST_BYTES + 1];
        assert!(matches!(
            decode_evaluation_request(&oversized),
            Err(ProtocolError::MessageTooLarge { .. })
        ));
        Ok(())
    }

    #[test]
    fn result_semantics_fail_closed() -> Result<(), Box<dyn Error>> {
        assert!(decode_evaluation_result(VALID_RESULT).is_ok());

        let mut invalid: Value = serde_json::from_slice(VALID_RESULT)?;
        invalid["verdict"] = json!("cancelled");
        assert!(matches!(
            decode_evaluation_result(&serde_json::to_vec(&invalid)?),
            Err(ProtocolError::SemanticViolation)
        ));

        let mut forged_mock: Value = serde_json::from_slice(VALID_RESULT)?;
        forged_mock["provenance"]["production_eligible"] = json!(true);
        assert!(matches!(
            decode_evaluation_result(&serde_json::to_vec(&forged_mock)?),
            Err(ProtocolError::SemanticViolation)
        ));

        let mut impossible_score: Value = serde_json::from_slice(VALID_RESULT)?;
        impossible_score["score"]["earned"] = json!(2);
        assert!(matches!(
            decode_evaluation_result(&serde_json::to_vec(&impossible_score)?),
            Err(ProtocolError::SemanticViolation)
        ));

        let mut forged_usage: Value = serde_json::from_slice(VALID_RESULT)?;
        forged_usage["usage"]["cpu_time_ms"] = json!(1);
        assert!(matches!(
            decode_evaluation_result(&serde_json::to_vec(&forged_usage)?),
            Err(ProtocolError::SemanticViolation)
        ));
        Ok(())
    }
}
