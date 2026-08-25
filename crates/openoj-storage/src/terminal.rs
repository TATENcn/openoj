use openoj_application::{
    CancelEvaluation, EvaluationSnapshot, JudgeSubmitResult, StoreError, SubmitResult,
};
use openoj_domain::{EvaluationIdentity, EvaluationStatus, IdempotencyKey};
use sqlx::{PgPool, Postgres, Row, Transaction};

pub async fn submit_result(
    pool: &PgPool,
    command: SubmitResult,
) -> Result<EvaluationSnapshot, StoreError> {
    let payload = openoj_protocol::encode_evaluation_result(&command.result)
        .map_err(|_| StoreError::CorruptData)?;
    let now = i64::try_from(command.now.value()).map_err(|_| StoreError::InvalidTime)?;
    let identity = command.result.identity();
    let mut transaction = pool.begin().await.map_err(|_| StoreError::Unavailable)?;
    let row = sqlx::query(
        "SELECT e.state AS evaluation_state, e.current_attempt_id, e.terminal_result, \
         e.result_idempotency_key, a.state AS attempt_state, a.lease_token, \
         a.lease_expires_at_ms, a.request_payload \
         FROM evaluations e \
         JOIN evaluation_attempts a ON a.attempt_id = e.current_attempt_id \
         WHERE e.evaluation_id = $1 FOR UPDATE OF e, a",
    )
    .bind(identity.evaluation_id().as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    .ok_or(StoreError::NotFound)?;

    let stored_result: Option<Vec<u8>> = row
        .try_get("terminal_result")
        .map_err(|_| StoreError::CorruptData)?;
    let stored_key: Option<String> = row
        .try_get("result_idempotency_key")
        .map_err(|_| StoreError::CorruptData)?;
    if let Some(stored_result) = stored_result {
        if stored_key.as_deref() == Some(command.idempotency_key.as_str()) {
            if stored_result != payload {
                return Err(StoreError::IdempotencyConflict);
            }
            transaction
                .commit()
                .await
                .map_err(|_| StoreError::Unavailable)?;
            return super::read::evaluation_status(pool, identity.evaluation_id().clone()).await;
        }
        return Err(StoreError::TerminalConflict);
    }

    let evaluation_state: String = row
        .try_get("evaluation_state")
        .map_err(|_| StoreError::CorruptData)?;
    let attempt_state: String = row
        .try_get("attempt_state")
        .map_err(|_| StoreError::CorruptData)?;
    let current_attempt_id: String = row
        .try_get("current_attempt_id")
        .map_err(|_| StoreError::CorruptData)?;
    if evaluation_state != "leased"
        || attempt_state != "leased"
        || current_attempt_id != identity.attempt_id().as_str()
    {
        return Err(StoreError::StaleLease);
    }
    let stored_lease_token: String = row
        .try_get("lease_token")
        .map_err(|_| StoreError::CorruptData)?;
    if stored_lease_token != command.lease_token.as_str() {
        return Err(StoreError::StaleLease);
    }
    let lease_expires_at: i64 = row
        .try_get("lease_expires_at_ms")
        .map_err(|_| StoreError::CorruptData)?;
    if lease_expires_at < now {
        return Err(StoreError::StaleLease);
    }
    let request_payload: Vec<u8> = row
        .try_get("request_payload")
        .map_err(|_| StoreError::CorruptData)?;
    let request = openoj_protocol::decode_evaluation_request(&request_payload)
        .map_err(|_| StoreError::CorruptData)?;
    if &EvaluationIdentity::from_request(&request) != identity {
        return Err(StoreError::IdentityConflict);
    }

    let state = command.result.status().as_str();
    let task_state = if command.result.status() == EvaluationStatus::Cancelled {
        "cancelled"
    } else {
        "completed"
    };
    write_terminal(
        &mut transaction,
        TerminalWrite {
            evaluation_id: identity.evaluation_id().as_str(),
            attempt_id: identity.attempt_id().as_str(),
            state,
            task_state,
            payload: &payload,
            idempotency_key: command.idempotency_key.as_str(),
            now,
        },
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|_| StoreError::Unavailable)?;
    super::read::evaluation_status(pool, identity.evaluation_id().clone()).await
}

pub async fn judge_submit_result(
    pool: &PgPool,
    command: JudgeSubmitResult,
) -> Result<EvaluationSnapshot, StoreError> {
    if command.result.provenance().node_id() != Some(&command.node_id) {
        return Err(StoreError::IdentityConflict);
    }
    let identity = command.result.identity();
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT a.node_id FROM evaluations e \
         JOIN evaluation_attempts a ON a.attempt_id = e.current_attempt_id \
         WHERE e.evaluation_id = $1",
    )
    .bind(identity.evaluation_id().as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    if owner.as_deref() != Some(command.node_id.as_str()) {
        return Err(StoreError::StaleLease);
    }
    let idempotency_key = IdempotencyKey::parse(command.operation_id.as_str())
        .map_err(|_| StoreError::CorruptData)?;
    submit_result(
        pool,
        SubmitResult {
            idempotency_key,
            lease_token: command.lease_token,
            result: command.result,
            now: command.now,
        },
    )
    .await
}

fn map_terminal_error(error: &sqlx::Error) -> StoreError {
    if error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23505")
    {
        StoreError::IdempotencyConflict
    } else {
        StoreError::Unavailable
    }
}

pub async fn cancel_evaluation(
    pool: &PgPool,
    command: CancelEvaluation,
) -> Result<EvaluationSnapshot, StoreError> {
    let now = i64::try_from(command.now.value()).map_err(|_| StoreError::InvalidTime)?;
    let mut transaction = pool.begin().await.map_err(|_| StoreError::Unavailable)?;
    let row = sqlx::query(
        "SELECT e.state AS evaluation_state, e.current_attempt_id, e.terminal_result, \
         e.result_idempotency_key, a.state AS attempt_state, a.request_payload \
         FROM evaluations e \
         JOIN evaluation_attempts a ON a.attempt_id = e.current_attempt_id \
         WHERE e.evaluation_id = $1 FOR UPDATE OF e, a",
    )
    .bind(command.evaluation_id.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    .ok_or(StoreError::NotFound)?;

    let request_payload: Vec<u8> = row
        .try_get("request_payload")
        .map_err(|_| StoreError::CorruptData)?;
    let request = openoj_protocol::decode_evaluation_request(&request_payload)
        .map_err(|_| StoreError::CorruptData)?;
    if request.evaluation_id() != &command.evaluation_id {
        return Err(StoreError::IdentityConflict);
    }
    let result =
        openoj_application::cancellation_result(&request).map_err(|_| StoreError::CorruptData)?;
    let payload =
        openoj_protocol::encode_evaluation_result(&result).map_err(|_| StoreError::CorruptData)?;

    let stored_result: Option<Vec<u8>> = row
        .try_get("terminal_result")
        .map_err(|_| StoreError::CorruptData)?;
    let stored_key: Option<String> = row
        .try_get("result_idempotency_key")
        .map_err(|_| StoreError::CorruptData)?;
    if let Some(stored_result) = stored_result {
        if stored_key.as_deref() == Some(command.idempotency_key.as_str()) {
            if stored_result != payload {
                return Err(StoreError::IdempotencyConflict);
            }
            transaction
                .commit()
                .await
                .map_err(|_| StoreError::Unavailable)?;
            return super::read::evaluation_status(pool, command.evaluation_id).await;
        }
        return Err(StoreError::TerminalConflict);
    }

    let evaluation_state: String = row
        .try_get("evaluation_state")
        .map_err(|_| StoreError::CorruptData)?;
    let attempt_state: String = row
        .try_get("attempt_state")
        .map_err(|_| StoreError::CorruptData)?;
    if !matches!(evaluation_state.as_str(), "queued" | "leased")
        || !matches!(attempt_state.as_str(), "queued" | "leased")
    {
        return Err(StoreError::InvalidTransition);
    }
    let current_attempt_id: String = row
        .try_get("current_attempt_id")
        .map_err(|_| StoreError::CorruptData)?;
    if current_attempt_id != result.identity().attempt_id().as_str() {
        return Err(StoreError::IdentityConflict);
    }
    if EvaluationIdentity::from_request(&request) != *result.identity() {
        return Err(StoreError::IdentityConflict);
    }

    write_terminal(
        &mut transaction,
        TerminalWrite {
            evaluation_id: command.evaluation_id.as_str(),
            attempt_id: &current_attempt_id,
            state: "cancelled",
            task_state: "cancelled",
            payload: &payload,
            idempotency_key: command.idempotency_key.as_str(),
            now,
        },
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|_| StoreError::Unavailable)?;
    super::read::evaluation_status(pool, command.evaluation_id).await
}

struct TerminalWrite<'a> {
    evaluation_id: &'a str,
    attempt_id: &'a str,
    state: &'a str,
    task_state: &'a str,
    payload: &'a [u8],
    idempotency_key: &'a str,
    now: i64,
}

async fn write_terminal(
    transaction: &mut Transaction<'_, Postgres>,
    write: TerminalWrite<'_>,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE evaluations SET state = $2, terminal_result = $3, \
         result_idempotency_key = $4, updated_at_ms = $5 WHERE evaluation_id = $1",
    )
    .bind(write.evaluation_id)
    .bind(write.state)
    .bind(write.payload)
    .bind(write.idempotency_key)
    .bind(write.now)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_terminal_error(&error))?;
    sqlx::query(
        "UPDATE evaluation_attempts SET state = $2, updated_at_ms = $3 WHERE attempt_id = $1",
    )
    .bind(write.attempt_id)
    .bind(write.state)
    .bind(write.now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    sqlx::query("UPDATE evaluation_tasks SET state = $2, updated_at_ms = $3 WHERE attempt_id = $1")
        .bind(write.attempt_id)
        .bind(write.task_state)
        .bind(write.now)
        .execute(&mut **transaction)
        .await
        .map_err(|_| StoreError::Unavailable)?;
    Ok(())
}
