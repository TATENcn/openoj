use openoj_application::{CreateEvaluation, EvaluationSnapshot, StoreError};
use openoj_domain::{ArtifactSensitivity, EvaluationRequest};
use openoj_protocol::encode_evaluation_request;
use sqlx::{PgPool, Postgres, Row, Transaction};

pub async fn create_evaluation(
    pool: &PgPool,
    command: CreateEvaluation,
) -> Result<EvaluationSnapshot, StoreError> {
    let payload =
        encode_evaluation_request(&command.request).map_err(|_| StoreError::CorruptData)?;
    let mut transaction = pool.begin().await.map_err(|_| StoreError::Unavailable)?;

    if let Some(row) = sqlx::query(
        "SELECT evaluation_id, initial_request FROM evaluations \
         WHERE creation_idempotency_key = $1 FOR UPDATE",
    )
    .bind(command.request.idempotency_key().as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?
    {
        let stored_payload: Vec<u8> = row
            .try_get("initial_request")
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

    register_artifact(&mut transaction, &command.request).await?;
    register_problem_version(&mut transaction, &command.request).await?;
    register_submission(&mut transaction, &command.request).await?;
    register_runtime(&mut transaction, &command.request).await?;

    let created_at =
        i64::try_from(command.created_at.value()).map_err(|_| StoreError::InvalidTime)?;
    let attempt_number = i64::from(command.request.attempt_number());
    insert_evaluation(&mut transaction, &command.request, &payload, created_at).await?;
    insert_attempt(
        &mut transaction,
        &command.request,
        &payload,
        attempt_number,
        created_at,
    )
    .await?;
    insert_task(&mut transaction, &command.request, created_at).await?;

    transaction
        .commit()
        .await
        .map_err(|_| StoreError::Unavailable)?;
    super::read::evaluation_status(pool, command.request.evaluation_id().clone()).await
}

pub(crate) async fn insert_task(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
    created_at: i64,
) -> Result<(), StoreError> {
    let capabilities = request
        .required_capabilities()
        .iter()
        .map(openoj_domain::Capability::as_str)
        .collect::<Vec<_>>();
    sqlx::query(
        "INSERT INTO evaluation_tasks \
         (attempt_id, state, required_capabilities, created_at_ms, updated_at_ms) \
         VALUES ($1, 'ready', $2, $3, $3)",
    )
    .bind(request.attempt_id().as_str())
    .bind(capabilities)
    .bind(created_at)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_insert_error(&error))?;
    Ok(())
}

async fn register_artifact(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
) -> Result<(), StoreError> {
    let source = request.submission().source();
    let sensitivity = match source.sensitivity() {
        ArtifactSensitivity::Public => "public",
        ArtifactSensitivity::Private => "private",
        ArtifactSensitivity::Hidden => "hidden",
    };
    let size = i64::try_from(source.size_bytes()).map_err(|_| StoreError::CorruptData)?;
    sqlx::query(
        "INSERT INTO artifacts (artifact_id, digest, media_type, size_bytes, sensitivity) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (artifact_id) DO NOTHING",
    )
    .bind(source.artifact_id().as_str())
    .bind(source.digest().as_str())
    .bind(source.media_type().as_str())
    .bind(size)
    .bind(sensitivity)
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;

    let stored: (String, String, i64, String) = sqlx::query_as(
        "SELECT digest, media_type, size_bytes, sensitivity FROM artifacts WHERE artifact_id = $1",
    )
    .bind(source.artifact_id().as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    if stored
        != (
            source.digest().as_str().to_owned(),
            source.media_type().as_str().to_owned(),
            size,
            sensitivity.to_owned(),
        )
    {
        return Err(StoreError::ImmutableReferenceConflict);
    }
    Ok(())
}

async fn register_problem_version(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
) -> Result<(), StoreError> {
    let reference = request.problem_version();
    sqlx::query(
        "INSERT INTO problem_versions (problem_version_id, problem_id, digest) \
         VALUES ($1, $2, $3) ON CONFLICT (problem_version_id) DO NOTHING",
    )
    .bind(reference.problem_version_id().as_str())
    .bind(reference.problem_id().as_str())
    .bind(reference.digest().as_str())
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    let stored: (String, String) = sqlx::query_as(
        "SELECT problem_id, digest FROM problem_versions WHERE problem_version_id = $1",
    )
    .bind(reference.problem_version_id().as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    if stored
        != (
            reference.problem_id().as_str().to_owned(),
            reference.digest().as_str().to_owned(),
        )
    {
        return Err(StoreError::ImmutableReferenceConflict);
    }
    Ok(())
}

async fn register_submission(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
) -> Result<(), StoreError> {
    let reference = request.submission();
    sqlx::query(
        "INSERT INTO submissions (submission_id, source_artifact_id) \
         VALUES ($1, $2) ON CONFLICT (submission_id) DO NOTHING",
    )
    .bind(reference.submission_id().as_str())
    .bind(reference.source().artifact_id().as_str())
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    let stored: String =
        sqlx::query_scalar("SELECT source_artifact_id FROM submissions WHERE submission_id = $1")
            .bind(reference.submission_id().as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;
    if stored != reference.source().artifact_id().as_str() {
        return Err(StoreError::ImmutableReferenceConflict);
    }
    Ok(())
}

async fn register_runtime(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
) -> Result<(), StoreError> {
    let reference = request.runtime();
    sqlx::query(
        "INSERT INTO runtimes (runtime_id, digest) VALUES ($1, $2) \
         ON CONFLICT (runtime_id) DO NOTHING",
    )
    .bind(reference.runtime_id().as_str())
    .bind(reference.digest().as_str())
    .execute(&mut **transaction)
    .await
    .map_err(|_| StoreError::Unavailable)?;
    let stored: String = sqlx::query_scalar("SELECT digest FROM runtimes WHERE runtime_id = $1")
        .bind(reference.runtime_id().as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| StoreError::Unavailable)?;
    if stored != reference.digest().as_str() {
        return Err(StoreError::ImmutableReferenceConflict);
    }
    Ok(())
}

async fn insert_evaluation(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
    payload: &[u8],
    created_at: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO evaluations \
         (evaluation_id, request_id, creation_idempotency_key, problem_version_id, submission_id, \
          runtime_id, initial_request, state, current_attempt_id, created_at_ms, updated_at_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued', $8, $9, $9)",
    )
    .bind(request.evaluation_id().as_str())
    .bind(request.request_id().as_str())
    .bind(request.idempotency_key().as_str())
    .bind(request.problem_version().problem_version_id().as_str())
    .bind(request.submission().submission_id().as_str())
    .bind(request.runtime().runtime_id().as_str())
    .bind(payload)
    .bind(request.attempt_id().as_str())
    .bind(created_at)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_insert_error(&error))?;
    Ok(())
}

async fn insert_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EvaluationRequest,
    payload: &[u8],
    attempt_number: i64,
    created_at: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO evaluation_attempts \
         (attempt_id, evaluation_id, request_id, attempt_number, attempt_idempotency_key, \
          request_payload, state, created_at_ms, updated_at_ms) \
         VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, $7)",
    )
    .bind(request.attempt_id().as_str())
    .bind(request.evaluation_id().as_str())
    .bind(request.request_id().as_str())
    .bind(attempt_number)
    .bind(request.idempotency_key().as_str())
    .bind(payload)
    .bind(created_at)
    .execute(&mut **transaction)
    .await
    .map_err(|error| map_insert_error(&error))?;
    Ok(())
}

fn map_insert_error(error: &sqlx::Error) -> StoreError {
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
