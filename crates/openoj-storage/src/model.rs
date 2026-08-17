use openoj_application::{EvaluationSnapshot, StoreError};
use openoj_domain::{AttemptId, AttemptState, EvaluationId, EvaluationState};

pub fn snapshot(
    evaluation_id: String,
    evaluation_state: &str,
    attempt_id: String,
    attempt_number: i64,
    attempt_state: &str,
    terminal_result: bool,
) -> Result<EvaluationSnapshot, StoreError> {
    Ok(EvaluationSnapshot {
        evaluation_id: EvaluationId::parse(evaluation_id).map_err(|_| StoreError::CorruptData)?,
        state: evaluation_state_from_str(evaluation_state)?,
        current_attempt_id: AttemptId::parse(attempt_id).map_err(|_| StoreError::CorruptData)?,
        attempt_number: u32::try_from(attempt_number).map_err(|_| StoreError::CorruptData)?,
        attempt_state: attempt_state_from_str(attempt_state)?,
        terminal_result,
    })
}

fn evaluation_state_from_str(value: &str) -> Result<EvaluationState, StoreError> {
    match value {
        "queued" => Ok(EvaluationState::Queued),
        "leased" => Ok(EvaluationState::Leased),
        "completed" => Ok(EvaluationState::Completed),
        "failed" => Ok(EvaluationState::Failed),
        "cancelled" => Ok(EvaluationState::Cancelled),
        _ => Err(StoreError::CorruptData),
    }
}

fn attempt_state_from_str(value: &str) -> Result<AttemptState, StoreError> {
    match value {
        "queued" => Ok(AttemptState::Queued),
        "leased" => Ok(AttemptState::Leased),
        "completed" => Ok(AttemptState::Completed),
        "failed" => Ok(AttemptState::Failed),
        "cancelled" => Ok(AttemptState::Cancelled),
        "expired" => Ok(AttemptState::Expired),
        _ => Err(StoreError::CorruptData),
    }
}
