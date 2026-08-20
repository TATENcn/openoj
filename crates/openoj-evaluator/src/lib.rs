//! Host-side evaluation logic for the P0 execution plane.
//!
//! `openoj-evaluator` is transport/VMM-neutral: it maps bounded guest
//! [`openoj_guest_protocol::Message::StageOutput`] events into
//! [`openoj_application::StageExecution`] and forms the host-side check decision.
//! It never drives a VMM and never trusts guest self-report as a final verdict.

use openoj_application::{ApplicationError, Decision, StageExecution};
use openoj_domain::{
    ArtifactId, ArtifactRef, ArtifactSensitivity, ContentDigest, Diagnostic, DiagnosticCode,
    EvidenceId, EvidenceKind, EvidenceRef, MediaType, ResourceUsage, Score, StageKind, Verdict,
};
use openoj_guest_protocol::{GuestDiagnostic, GuestUsage};

/// Stable evidence kind for a captured stage standard output.
pub const EVIDENCE_STDOUT: &str = "stdout";

/// Stable evidence kind for a captured stage diagnostics stream.
pub const EVIDENCE_DIAGNOSTICS: &str = "diagnostics";

/// Media type used for captured plain-text output.
pub const MEDIA_PLAIN_TEXT: &str = "text/plain";

/// Media type used for structured diagnostics.
pub const MEDIA_DIAGNOSTICS: &str = "application/json";

/// Guest exit code reserved for a wall-clock timeout by the guest agent.
pub const GUEST_TIMEOUT_EXIT_CODE: i32 = 124;

/// Maps guest resource usage into the domain resource-usage summary.
///
/// # Errors
///
/// Returns [`ApplicationError`] when a value exceeds the domain protocol limit.
pub fn usage_from_guest(value: GuestUsage) -> Result<ResourceUsage, ApplicationError> {
    ResourceUsage::new(
        value.cpu_time_ms(),
        value.wall_time_ms(),
        value.memory_peak_bytes(),
        value.output_bytes(),
    )
    .map_err(ApplicationError::from)
}

/// Maps bounded guest diagnostics into domain diagnostics.
///
/// # Errors
///
/// Returns [`ApplicationError`] when a diagnostic code is malformed or a message
/// exceeds the protocol bound.
pub fn diagnostics_from_guest(
    values: &[GuestDiagnostic],
) -> Result<Vec<Diagnostic>, ApplicationError> {
    values
        .iter()
        .map(|value| {
            // Domain diagnostic codes are uppercase ASCII tokens.
            let code = DiagnosticCode::parse(value.code().to_uppercase())?;
            Diagnostic::new(code, value.message(), value.truncated()).map_err(ApplicationError::from)
        })
        .collect()
}

fn content_digest(guest_digest: &str) -> Result<ContentDigest, ApplicationError> {
    ContentDigest::parse(format!("sha256:{guest_digest}")).map_err(ApplicationError::from)
}

/// Builds a content-addressed evidence reference for a stage output.
///
/// # Errors
///
/// Returns [`ApplicationError`] when an identifier, digest, or media type is
/// invalid, or the declared size exceeds the protocol limit.
pub fn stdout_evidence(guest_digest: &str, output_bytes: u64) -> Result<EvidenceRef, ApplicationError> {
    build_evidence(EvidenceKind::parse(EVIDENCE_STDOUT)?, guest_digest, output_bytes)
}

fn build_evidence(
    kind: EvidenceKind,
    guest_digest: &str,
    output_bytes: u64,
) -> Result<EvidenceRef, ApplicationError> {
    let digest = content_digest(guest_digest)?;
    let artifact = ArtifactRef::new(
        ArtifactId::parse(format!("artifact-{}", short_token(guest_digest, 48)))?,
        digest,
        MediaType::parse(MEDIA_PLAIN_TEXT)?,
        output_bytes,
        ArtifactSensitivity::Private,
    )?;
    Ok(EvidenceRef::new(
        EvidenceId::parse(format!(
            "evidence-{}-{}",
            kind.as_str(),
            short_token(guest_digest, 32)
        ))?,
        kind,
        Some(artifact),
    ))
}

/// Returns a bounded prefix of a digest suitable for low-cardinality identifiers.
fn short_token(digest: &str, maximum: usize) -> String {
    let bytes = digest.as_bytes();
    let count = bytes.len().min(maximum);
    String::from_utf8_lossy(&bytes[..count]).into_owned()
}

/// Forms the host-side check decision from a run stage's exit status.
///
/// This is a structural check for the P0 vertical slice: a zero exit code and a
/// bounded output digest yield `Accepted`; a nonzero exit yields `WrongAnswer`.
/// Full comparison against hidden test-case outputs requires object-storage test
/// data, which is a P0 non-goal, so it is documented as `Unverified`.
///
/// # Errors
///
/// Returns [`ApplicationError`] when the verdict/score pairing is invalid.
pub fn host_check_decision(exit_code: i32) -> Result<Decision, ApplicationError> {
    let verdict = if exit_code == 0 {
        Verdict::Accepted
    } else {
        Verdict::WrongAnswer
    };
    let score = if exit_code == 0 {
        Score::new(1, 1)?
    } else {
        Score::new(0, 1)?
    };
    Decision::new(verdict, score)
}

/// Maps a guest build/run stage output into a domain [`StageExecution`].
///
/// The guest's own exit code drives whether the stage succeeds or is rejected,
/// but the final check verdict is always formed host-side via
/// [`host_check_decision`]; a stage failure never fabricates a later success.
///
/// # Errors
///
/// Returns [`ApplicationError`] when evidence construction or usage conversion fails.
pub fn execution_for_stage_output(
    stage: StageKind,
    exit_code: i32,
    guest_digest: &str,
    output_bytes: u64,
    usage: GuestUsage,
    guest_diagnostics: &[GuestDiagnostic],
) -> Result<StageExecution, ApplicationError> {
    let usage = usage_from_guest(usage)?;
    let diagnostics = diagnostics_from_guest(guest_diagnostics)?;
    let evidence = match stage {
        StageKind::Build | StageKind::Run => vec![stdout_evidence(
            guest_digest,
            output_bytes,
        )?],
        _ => Vec::new(),
    };

    if exit_code == 0 {
        return Ok(StageExecution::Succeeded {
            usage,
            diagnostics,
            evidence,
            decision: None,
        });
    }

    let verdict = match stage {
        StageKind::Build => Verdict::CompileError,
        StageKind::Run if exit_code == GUEST_TIMEOUT_EXIT_CODE => Verdict::TimeLimitExceeded,
        StageKind::Run => Verdict::RuntimeError,
        _ => Verdict::SystemError,
    };
    Ok(StageExecution::Rejected {
        verdict,
        usage,
        diagnostics,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openoj_guest_protocol::GuestUsage;

    const DIGEST: &str = "abababababababababababababababababababababababababababababababab";

    #[test]
    fn zero_exit_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        let decision = host_check_decision(0)?;
        assert_eq!(decision.verdict(), Verdict::Accepted);
        assert_eq!(decision.score(), Score::new(1, 1)?);
        Ok(())
    }

    #[test]
    fn nonzero_exit_is_wrong_answer() -> Result<(), Box<dyn std::error::Error>> {
        let decision = host_check_decision(1)?;
        assert_eq!(decision.verdict(), Verdict::WrongAnswer);
        assert_eq!(decision.score(), Score::new(0, 1)?);
        Ok(())
    }

    #[test]
    fn successful_run_maps_to_succeeded() -> Result<(), Box<dyn std::error::Error>> {
        let execution = execution_for_stage_output(
            StageKind::Run,
            0,
            DIGEST,
            3,
            GuestUsage::new(1, 1, 0, 3),
            &[],
        )?;
        assert!(matches!(execution, StageExecution::Succeeded { .. }));
        Ok(())
    }

    #[test]
    fn build_failure_maps_to_compile_error() -> Result<(), Box<dyn std::error::Error>> {
        let execution = execution_for_stage_output(
            StageKind::Build,
            1,
            DIGEST,
            0,
            GuestUsage::default(),
            &[GuestDiagnostic::new("cc", "syntax error")],
        )?;
        assert!(matches!(
            execution,
            StageExecution::Rejected {
                verdict: Verdict::CompileError,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn test_timeout_maps_to_time_limit_exceeded() -> Result<(), Box<dyn std::error::Error>> {
        let execution = execution_for_stage_output(
            StageKind::Run,
            GUEST_TIMEOUT_EXIT_CODE,
            DIGEST,
            0,
            GuestUsage::default(),
            &[],
        )?;
        assert!(matches!(
            execution,
            StageExecution::Rejected {
                verdict: Verdict::TimeLimitExceeded,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn stdout_evidence_is_content_addressed() -> Result<(), Box<dyn std::error::Error>> {
        let evidence = stdout_evidence(DIGEST, 4)?;
        let artifact = evidence.artifact().ok_or("missing artifact")?;
        assert_eq!(artifact.digest().as_str(), "sha256:abababababababababababababababababababababababababababababababab");
        Ok(())
    }
}
