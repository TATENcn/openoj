use openoj_application::{EvaluationSnapshot, StoreError};
use openoj_domain::EvaluationId;
use sqlx::{PgPool, Row};

pub async fn evaluation_status(
    pool: &PgPool,
    evaluation_id: EvaluationId,
) -> Result<EvaluationSnapshot, StoreError> {
    let row = sqlx::query(
        "SELECT e.evaluation_id, e.state AS evaluation_state, \
         a.attempt_id, a.attempt_number, a.state AS attempt_state, \
         a.request_payload, \
         e.terminal_result IS NOT NULL AS terminal_result \
         FROM evaluations e \
         JOIN evaluation_attempts a ON a.attempt_id = e.current_attempt_id \
         WHERE e.evaluation_id = $1",
    )
    .bind(evaluation_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| StoreError::Unavailable)?
    .ok_or(StoreError::NotFound)?;

    let stored_evaluation_id: String = row
        .try_get("evaluation_id")
        .map_err(|_| StoreError::CorruptData)?;
    let stored_attempt_id: String = row
        .try_get("attempt_id")
        .map_err(|_| StoreError::CorruptData)?;
    let stored_attempt_number: i64 = row
        .try_get("attempt_number")
        .map_err(|_| StoreError::CorruptData)?;
    let request_payload: Vec<u8> = row
        .try_get("request_payload")
        .map_err(|_| StoreError::CorruptData)?;
    let request = openoj_protocol::decode_evaluation_request(&request_payload)
        .map_err(|_| StoreError::CorruptData)?;
    if request.evaluation_id().as_str() != stored_evaluation_id
        || request.attempt_id().as_str() != stored_attempt_id
        || i64::from(request.attempt_number()) != stored_attempt_number
    {
        return Err(StoreError::CorruptData);
    }

    super::model::snapshot(
        stored_evaluation_id,
        &row.try_get::<String, _>("evaluation_state")
            .map_err(|_| StoreError::CorruptData)?,
        stored_attempt_id,
        stored_attempt_number,
        &row.try_get::<String, _>("attempt_state")
            .map_err(|_| StoreError::CorruptData)?,
        row.try_get("terminal_result")
            .map_err(|_| StoreError::CorruptData)?,
    )
}
