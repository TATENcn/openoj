use openoj_application::{
    ClaimTask, EvaluationSnapshot, JudgeClaim, JudgeRenew, JudgeRenewDirective, RetryExpired,
    StoreError, TaskLease,
};
use openoj_domain::EvaluationRequest;
use sqlx::{PgPool, Postgres, Row, Transaction};

pub async fn claim_task(pool: &PgPool, command: ClaimTask) -> Result<TaskLease, StoreError> {
    claim_task_with_capabilities(pool, command, None, None).await
}

pub async fn judge_claim_task(pool: &PgPool, command: JudgeClaim) -> Result<TaskLease, StoreError> {
    if let Some(replayed) = claim_replay(pool, &command).await? {
        return Ok(replayed);
    }
    let claim = ClaimTask {
        node_id: command.node_id.clone(),
        lease_token: command.lease_token.clone(),
        now: command.now,
        lease_duration: command.lease_policy.lease_duration(),
    };
    let capabilities = command
        .declared_capabilities
        .iter()
        .map(openoj_domain::Capability::as_str)
        .collect::<Vec<_>>();
    claim_task_with_capabilities(
        pool,
        claim,
        Some(command.operation_id.as_str()),
        Some(capabilities),
    )
    .await
}

async fn claim_replay(
    pool: &PgPool,
    command: &JudgeClaim,
) -> Result<Option<TaskLease>, StoreError> {
    let row = sqlx::query(
        "SELECT node_id, lease_token, lease_expires_at_ms, request_payload, state \
         FROM evaluation_attempts WHERE claim_operation_id = $1",
    )
    .bind(command.operation_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let node_id: String = row
        .try_get("node_id")
        .map_err(|_| StoreError::CorruptData)?;
    if node_id != command.node_id.as_str() {
        return Err(StoreError::IdempotencyConflict);
    }
    let state: String = row.try_get("state").map_err(|_| StoreError::CorruptData)?;
    if state != "leased" {
        return Err(StoreError::LeaseConflict);
    }
    let lease_token: String = row
        .try_get("lease_token")
        .map_err(|_| StoreError::CorruptData)?;
    let expires_at: i64 = row
        .try_get("lease_expires_at_ms")
        .map_err(|_| StoreError::CorruptData)?;
    let payload: Vec<u8> = row
        .try_get("request_payload")
        .map_err(|_| StoreError::CorruptData)?;
    let request = openoj_protocol::decode_evaluation_request(&payload)
        .map_err(|_| StoreError::CorruptData)?;
    let lease_token =
        openoj_domain::LeaseToken::parse(lease_token).map_err(|_| StoreError::CorruptData)?;
    let expires_at = u64::try_from(expires_at)
        .ok()
        .and_then(|value| openoj_domain::UnixMillis::new(value).ok())
        .ok_or(StoreError::CorruptData)?;
    Ok(Some(TaskLease {
        request,
        node_id: command.node_id.clone(),
        lease_token,
        expires_at,
    }))
}

pub async fn judge_renew_lease(
    pool: &PgPool,
    command: JudgeRenew,
) -> Result<JudgeRenewDirective, StoreError> {
    let expires_at = command
        .now
        .checked_add(command.lease_policy.lease_duration())
        .map_err(|_| StoreError::InvalidTime)?;
    let now = i64::try_from(command.now.value()).map_err(|_| StoreError::InvalidTime)?;
    let expiry = i64::try_from(expires_at.value()).map_err(|_| StoreError::InvalidTime)?;
    let mut transaction = pool.begin().await.map_err(|_| StoreError::Unavailable)?;
    let row = sqlx::query(
        "SELECT e.state AS evaluation_state, e.current_attempt_id, a.state AS attempt_state, \
         a.node_id, a.lease_token FROM evaluations e \
         JOIN evaluation_attempts a ON a.attempt_id = e.current_attempt_id \
         WHERE e.evaluation_id = $1 FOR UPDATE OF e, a",
    )
    .bind(command.evaluation_id.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    .ok_or(StoreError::NotFound)?;
    let evaluation_state: String = row
        .try_get("evaluation_state")
        .map_err(|_| StoreError::CorruptData)?;
    let attempt_state: String = row
        .try_get("attempt_state")
        .map_err(|_| StoreError::CorruptData)?;
    if evaluation_state == "cancelled" || attempt_state == "cancelled" {
        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Unavailable)?;
        return Ok(JudgeRenewDirective::Cancel);
    }
    let attempt_id: String = row
        .try_get("current_attempt_id")
        .map_err(|_| StoreError::CorruptData)?;
    let node_id: String = row
        .try_get("node_id")
        .map_err(|_| StoreError::CorruptData)?;
    let lease_token: String = row
        .try_get("lease_token")
        .map_err(|_| StoreError::CorruptData)?;
    if evaluation_state != "leased"
        || attempt_state != "leased"
        || attempt_id != command.attempt_id.as_str()
        || node_id != command.node_id.as_str()
        || lease_token != command.lease_token.as_str()
    {
        return Err(StoreError::StaleLease);
    }
    sqlx::query(
        "UPDATE evaluation_attempts SET lease_expires_at_ms = $2, updated_at_ms = $3 \
         WHERE attempt_id = $1",
    )
    .bind(command.attempt_id.as_str())
    .bind(expiry)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    transaction
        .commit()
        .await
        .map_err(|_| StoreError::Unavailable)?;
    Ok(JudgeRenewDirective::Continue { expires_at })
}

async fn claim_task_with_capabilities(
    pool: &PgPool,
    command: ClaimTask,
    operation_id: Option<&str>,
    capabilities: Option<Vec<&str>>,
) -> Result<TaskLease, StoreError> {
    let expires_at = command
        .now
        .checked_add(command.lease_duration)
        .map_err(|_| StoreError::InvalidTime)?;
    let now = i64::try_from(command.now.value()).map_err(|_| StoreError::InvalidTime)?;
    let expiry = i64::try_from(expires_at.value()).map_err(|_| StoreError::InvalidTime)?;
    let mut transaction = pool.begin().await.map_err(|_| StoreError::Unavailable)?;

    let row = sqlx::query(
        "SELECT t.attempt_id, a.evaluation_id, a.attempt_number, a.request_payload \
         FROM evaluation_tasks t \
         JOIN evaluation_attempts a ON a.attempt_id = t.attempt_id \
         JOIN evaluations e ON e.current_attempt_id = a.attempt_id \
         WHERE t.state = 'ready' AND a.state = 'queued' AND e.state = 'queued' \
         AND ($1::text[] IS NULL OR t.required_capabilities <@ $1) \
         ORDER BY t.created_at_ms, t.attempt_id \
         FOR UPDATE OF t SKIP LOCKED LIMIT 1",
    )
    .bind(capabilities)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    .ok_or(StoreError::NoTaskAvailable)?;

    let attempt_id: String = row
        .try_get("attempt_id")
        .map_err(|_| StoreError::CorruptData)?;
    let evaluation_id: String = row
        .try_get("evaluation_id")
        .map_err(|_| StoreError::CorruptData)?;
    let attempt_number: i64 = row
        .try_get("attempt_number")
        .map_err(|_| StoreError::CorruptData)?;
    let payload: Vec<u8> = row
        .try_get("request_payload")
        .map_err(|_| StoreError::CorruptData)?;
    let request = openoj_protocol::decode_evaluation_request(&payload)
        .map_err(|_| StoreError::CorruptData)?;
    if request.attempt_id().as_str() != attempt_id
        || request.evaluation_id().as_str() != evaluation_id
        || i64::from(request.attempt_number()) != attempt_number
    {
        return Err(StoreError::CorruptData);
    }

    sqlx::query(
        "UPDATE evaluation_attempts SET state = 'leased', node_id = $2, lease_token = $3, \
         lease_expires_at_ms = $4, claim_operation_id = $5, updated_at_ms = $6 WHERE attempt_id = $1",
    )
    .bind(&attempt_id)
    .bind(command.node_id.as_str())
    .bind(command.lease_token.as_str())
    .bind(expiry)
    .bind(operation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    sqlx::query(
        "UPDATE evaluations SET state = 'leased', updated_at_ms = $2 \
         WHERE evaluation_id = $1",
    )
    .bind(&evaluation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    sqlx::query(
        "UPDATE evaluation_tasks SET state = 'leased', updated_at_ms = $2 WHERE attempt_id = $1",
    )
    .bind(&attempt_id)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    transaction
        .commit()
        .await
        .map_err(|_| StoreError::Unavailable)?;

    Ok(TaskLease {
        request,
        node_id: command.node_id,
        lease_token: command.lease_token,
        expires_at,
    })
}

pub async fn retry_expired(
    pool: &PgPool,
    command: RetryExpired,
) -> Result<EvaluationSnapshot, StoreError> {
    let now = i64::try_from(command.now.value()).map_err(|_| StoreError::InvalidTime)?;
    let payload = openoj_protocol::encode_evaluation_request(&command.request)
        .map_err(|_| StoreError::CorruptData)?;
    let mut transaction = pool.begin().await.map_err(|_| StoreError::Unavailable)?;
    if let Some(row) = sqlx::query(
        "SELECT evaluation_id, request_payload FROM evaluation_attempts \
         WHERE attempt_idempotency_key = $1 FOR UPDATE",
    )
    .bind(command.request.idempotency_key().as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    {
        let stored_payload: Vec<u8> = row
            .try_get("request_payload")
            .map_err(|_| StoreError::CorruptData)?;
        if stored_payload != payload {
            return Err(StoreError::IdempotencyConflict);
        }
        let evaluation_id: String = row
            .try_get("evaluation_id")
            .map_err(|_| StoreError::CorruptData)?;
        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let evaluation_id = openoj_domain::EvaluationId::parse(evaluation_id)
            .map_err(|_| StoreError::CorruptData)?;
        return super::read::evaluation_status(pool, evaluation_id).await;
    }
    let row = sqlx::query(
        "SELECT e.state AS evaluation_state, e.current_attempt_id, \
         a.state AS attempt_state, a.attempt_number, a.lease_expires_at_ms, a.request_payload \
         FROM evaluations e \
         JOIN evaluation_attempts a ON a.attempt_id = e.current_attempt_id \
         WHERE e.evaluation_id = $1 FOR UPDATE OF e, a",
    )
    .bind(command.request.evaluation_id().as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    .ok_or(StoreError::NotFound)?;

    let evaluation_state: String = row
        .try_get("evaluation_state")
        .map_err(|_| StoreError::CorruptData)?;
    let attempt_state: String = row
        .try_get("attempt_state")
        .map_err(|_| StoreError::CorruptData)?;
    if evaluation_state != "leased" || attempt_state != "leased" {
        return Err(StoreError::InvalidTransition);
    }
    let current_attempt_id: String = row
        .try_get("current_attempt_id")
        .map_err(|_| StoreError::CorruptData)?;
    let current_attempt_number: i64 = row
        .try_get("attempt_number")
        .map_err(|_| StoreError::CorruptData)?;
    let lease_expires_at: i64 = row
        .try_get("lease_expires_at_ms")
        .map_err(|_| StoreError::CorruptData)?;
    if lease_expires_at >= now {
        return Err(StoreError::LeaseConflict);
    }
    if i64::from(command.request.attempt_number()) != current_attempt_number + 1 {
        return Err(StoreError::InvalidTransition);
    }

    let current_payload: Vec<u8> = row
        .try_get("request_payload")
        .map_err(|_| StoreError::CorruptData)?;
    let current_request = openoj_protocol::decode_evaluation_request(&current_payload)
        .map_err(|_| StoreError::CorruptData)?;
    if !same_evaluation_semantics(&current_request, &command.request) {
        return Err(StoreError::IdentityConflict);
    }

    replace_attempt(
        &mut transaction,
        &current_attempt_id,
        &command.request,
        &payload,
        now,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|_| StoreError::Unavailable)?;
    super::read::evaluation_status(pool, command.request.evaluation_id().clone()).await
}

async fn replace_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    current_attempt_id: &str,
    request: &EvaluationRequest,
    payload: &[u8],
    now: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE evaluation_attempts SET state = 'expired', updated_at_ms = $2 \
         WHERE attempt_id = $1",
    )
    .bind(current_attempt_id)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    sqlx::query(
        "UPDATE evaluation_tasks SET state = 'expired', updated_at_ms = $2 WHERE attempt_id = $1",
    )
    .bind(current_attempt_id)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    sqlx::query(
        "UPDATE evaluations SET state = 'queued', current_attempt_id = $2, \
         updated_at_ms = $3 WHERE evaluation_id = $1",
    )
    .bind(request.evaluation_id().as_str())
    .bind(request.attempt_id().as_str())
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    sqlx::query(
        "INSERT INTO evaluation_attempts \
         (attempt_id, evaluation_id, request_id, attempt_number, attempt_idempotency_key, \
          request_payload, state, created_at_ms, updated_at_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, $7)",
    )
    .bind(request.attempt_id().as_str())
    .bind(request.evaluation_id().as_str())
    .bind(request.request_id().as_str())
    .bind(i64::from(request.attempt_number()))
    .bind(request.idempotency_key().as_str())
    .bind(payload)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_retry_insert_error(&error))?;
    super::create::insert_task(transaction, request, now).await?;
    Ok(())
}

fn same_evaluation_semantics(
    current: &openoj_domain::EvaluationRequest,
    retry: &openoj_domain::EvaluationRequest,
) -> bool {
    current.evaluation_id() == retry.evaluation_id()
        && current.problem_version() == retry.problem_version()
        && current.submission() == retry.submission()
        && current.runtime() == retry.runtime()
        && current.plan() == retry.plan()
        && current.policy() == retry.policy()
        && current.required_capabilities() == retry.required_capabilities()
}

fn map_retry_insert_error(error: &sqlx::Error) -> StoreError {
    if error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23505")
    {
        StoreError::IdentityConflict
    } else {
        StoreError::Unavailable
    }
}
