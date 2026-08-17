use std::error::Error;

use openoj_application::{
    CancelEvaluation, ClaimTask, ControlPlane, CreateEvaluation, Decision, JudgeClaim, LeasePolicy,
    RetryExpired, StageContext, StageExecution, StageExecutor, StoreError, SubmitResult,
};
use openoj_domain::{
    AttemptState, Capability, ClaimOperationId, EvaluationState, ExecutorKind, IdempotencyKey,
    LeaseDuration, LeaseToken, NodeId, ResourceUsage, Score, StageKind, UnixMillis, Verdict,
};
use openoj_protocol::{decode_evaluation_request, encode_evaluation_request};
use openoj_storage::PostgresEvaluationStore;
use sqlx::PgPool;

const VALID_REQUEST: &[u8] =
    include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json");

fn distinct_request_value(label: &str) -> Result<serde_json::Value, serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_slice(VALID_REQUEST)?;
    value["request_id"] = format!("req_{label}").into();
    value["idempotency_key"] = format!("idem_{label}").into();
    value["evaluation_id"] = format!("eval_{label}").into();
    value["attempt_id"] = format!("attempt_{label}").into();
    Ok(value)
}

struct DecisionExecutor {
    decision: Decision,
}

struct CancellationExecutor;

impl StageExecutor for CancellationExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::DevelopmentMock
    }

    fn production_eligible(&self) -> bool {
        false
    }

    fn node_id(&self) -> Option<NodeId> {
        None
    }

    fn supports(&self, capability: &Capability) -> bool {
        capability.as_str() == "algorithm.batch"
    }

    fn execute(&mut self, _context: StageContext<'_>) -> StageExecution {
        StageExecution::Cancelled {
            usage: ResourceUsage::default(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

impl StageExecutor for DecisionExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::DevelopmentMock
    }

    fn production_eligible(&self) -> bool {
        false
    }

    fn node_id(&self) -> Option<NodeId> {
        None
    }

    fn supports(&self, capability: &Capability) -> bool {
        capability.as_str() == "algorithm.batch"
    }

    fn execute(&mut self, context: StageContext<'_>) -> StageExecution {
        StageExecution::Succeeded {
            usage: ResourceUsage::default(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
            decision: (context.stage() == StageKind::Check).then_some(self.decision),
        }
    }
}

#[sqlx::test(migrations = false)]
async fn migration_is_repeatable_and_rejects_a_newer_schema(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());

    store.migrate().await?;
    store.migrate().await?;

    let schema_version: i32 =
        sqlx::query_scalar("SELECT schema_version FROM openoj_schema_metadata WHERE singleton")
            .fetch_one(&pool)
            .await?;
    assert_eq!(schema_version, 2);
    store.check_compatibility().await?;

    let maximum_media_type = format!("{}/{}", "a".repeat(64), "b".repeat(64));
    sqlx::query(
        "INSERT INTO artifacts \
         (artifact_id, digest, media_type, size_bytes, sensitivity) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("artifact_boundary")
    .bind(format!("sha256:{}", "a".repeat(64)))
    .bind(maximum_media_type)
    .bind(1_099_511_627_776_i64)
    .bind("hidden")
    .execute(&pool)
    .await?;

    sqlx::query("UPDATE openoj_schema_metadata SET schema_version = 3 WHERE singleton")
        .execute(&pool)
        .await?;
    assert_eq!(
        store.check_compatibility().await,
        Err(StoreError::IncompatibleSchema)
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn create_is_atomic_and_same_payload_replay_is_idempotent(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let control = ControlPlane::new(store);
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let command = CreateEvaluation {
        request: request.clone(),
        created_at: UnixMillis::new(1_000)?,
    };

    let created = control.create_evaluation(command.clone()).await?;
    let replayed = control.create_evaluation(command).await?;

    assert_eq!(created, replayed);
    assert_eq!(created.evaluation_id, request.evaluation_id().clone());
    assert_eq!(created.state, EvaluationState::Queued);
    assert_eq!(created.current_attempt_id, request.attempt_id().clone());
    assert_eq!(created.attempt_number, 1);
    assert_eq!(created.attempt_state, AttemptState::Queued);
    assert!(!created.terminal_result);

    let evaluations: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluations")
        .fetch_one(&pool)
        .await?;
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluation_attempts")
        .fetch_one(&pool)
        .await?;
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluation_tasks")
        .fetch_one(&pool)
        .await?;
    assert_eq!((evaluations, attempts, tasks), (1, 1, 1));
    let required_capabilities: Vec<String> = sqlx::query_scalar(
        "SELECT required_capabilities FROM evaluation_tasks WHERE attempt_id = $1",
    )
    .bind(request.attempt_id().as_str())
    .fetch_one(&pool)
    .await?;
    assert_eq!(required_capabilities, vec!["algorithm.batch"]);
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn create_rejects_idempotency_identity_and_immutable_reference_conflicts(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let control = ControlPlane::new(store);
    let original = decode_evaluation_request(VALID_REQUEST)?;
    control
        .create_evaluation(CreateEvaluation {
            request: original,
            created_at: UnixMillis::new(1_000)?,
        })
        .await?;

    let mut same_key_different_payload: serde_json::Value = serde_json::from_slice(VALID_REQUEST)?;
    same_key_different_payload["request_id"] = "req_conflict".into();
    let request = decode_evaluation_request(&serde_json::to_vec(&same_key_different_payload)?)?;
    assert_eq!(
        control
            .create_evaluation(CreateEvaluation {
                request,
                created_at: UnixMillis::new(1_001)?,
            })
            .await,
        Err(StoreError::IdempotencyConflict)
    );

    let mut same_identity_different_key: serde_json::Value = serde_json::from_slice(VALID_REQUEST)?;
    same_identity_different_key["request_id"] = "req_identity_conflict".into();
    same_identity_different_key["idempotency_key"] = "idem_identity_conflict".into();
    same_identity_different_key["attempt_id"] = "attempt_identity_conflict".into();
    let request = decode_evaluation_request(&serde_json::to_vec(&same_identity_different_key)?)?;
    assert_eq!(
        control
            .create_evaluation(CreateEvaluation {
                request,
                created_at: UnixMillis::new(1_002)?,
            })
            .await,
        Err(StoreError::IdentityConflict)
    );

    let mut immutable_reference_conflict: serde_json::Value =
        serde_json::from_slice(VALID_REQUEST)?;
    immutable_reference_conflict["request_id"] = "req_immutable_conflict".into();
    immutable_reference_conflict["idempotency_key"] = "idem_immutable_conflict".into();
    immutable_reference_conflict["evaluation_id"] = "eval_immutable_conflict".into();
    immutable_reference_conflict["attempt_id"] = "attempt_immutable_conflict".into();
    immutable_reference_conflict["submission"]["submission_id"] =
        "submission_immutable_conflict".into();
    immutable_reference_conflict["submission"]["source"]["digest"] =
        format!("sha256:{}", "f".repeat(64)).into();
    let request = decode_evaluation_request(&serde_json::to_vec(&immutable_reference_conflict)?)?;
    assert_eq!(
        control
            .create_evaluation(CreateEvaluation {
                request,
                created_at: UnixMillis::new(1_003)?,
            })
            .await,
        Err(StoreError::ImmutableReferenceConflict)
    );

    let evaluations: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluations")
        .fetch_one(&pool)
        .await?;
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluation_attempts")
        .fetch_one(&pool)
        .await?;
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluation_tasks")
        .fetch_one(&pool)
        .await?;
    assert_eq!((evaluations, attempts, tasks), (1, 1, 1));
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn every_immutable_reference_rejects_metadata_drift(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool);
    store.migrate().await?;
    let control = ControlPlane::new(store);
    control
        .create_evaluation(CreateEvaluation {
            request: decode_evaluation_request(VALID_REQUEST)?,
            created_at: UnixMillis::new(2_000)?,
        })
        .await?;

    let mut problem = distinct_request_value("problem_drift")?;
    problem["problem_version"]["digest"] = format!("sha256:{}", "a".repeat(64)).into();
    assert_eq!(
        control
            .create_evaluation(CreateEvaluation {
                request: decode_evaluation_request(&serde_json::to_vec(&problem)?)?,
                created_at: UnixMillis::new(2_001)?,
            })
            .await,
        Err(StoreError::ImmutableReferenceConflict)
    );

    let mut submission = distinct_request_value("submission_drift")?;
    submission["submission"]["source"]["artifact_id"] = "artifact_source_drift".into();
    submission["submission"]["source"]["digest"] = format!("sha256:{}", "b".repeat(64)).into();
    assert_eq!(
        control
            .create_evaluation(CreateEvaluation {
                request: decode_evaluation_request(&serde_json::to_vec(&submission)?)?,
                created_at: UnixMillis::new(2_002)?,
            })
            .await,
        Err(StoreError::ImmutableReferenceConflict)
    );

    let mut runtime = distinct_request_value("runtime_drift")?;
    runtime["runtime"]["digest"] = format!("sha256:{}", "c".repeat(64)).into();
    assert_eq!(
        control
            .create_evaluation(CreateEvaluation {
                request: decode_evaluation_request(&serde_json::to_vec(&runtime)?)?,
                created_at: UnixMillis::new(2_003)?,
            })
            .await,
        Err(StoreError::ImmutableReferenceConflict)
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn status_rejects_a_corrupt_persisted_request(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let control = ControlPlane::new(store);
    let request = decode_evaluation_request(VALID_REQUEST)?;
    control
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(3_000)?,
        })
        .await?;

    sqlx::query("UPDATE evaluation_attempts SET request_payload = $1 WHERE attempt_id = $2")
        .bind(b"{}".as_slice())
        .bind(request.attempt_id().as_str())
        .execute(&pool)
        .await?;

    assert_eq!(
        control
            .evaluation_status(request.evaluation_id().clone())
            .await,
        Err(StoreError::CorruptData)
    );

    let mismatched = decode_evaluation_request(&serde_json::to_vec(&distinct_request_value(
        "stored_identity_drift",
    )?)?)?;
    sqlx::query("UPDATE evaluation_attempts SET request_payload = $1 WHERE attempt_id = $2")
        .bind(encode_evaluation_request(&mismatched)?)
        .bind(request.attempt_id().as_str())
        .execute(&pool)
        .await?;
    assert_eq!(
        control
            .evaluation_status(request.evaluation_id().clone())
            .await,
        Err(StoreError::CorruptData)
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn concurrent_claim_has_one_winner_and_expiry_creates_attempt_two(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let request = decode_evaluation_request(VALID_REQUEST)?;
    ControlPlane::new(store.clone())
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(4_000)?,
        })
        .await?;

    let first = ControlPlane::new(store.clone());
    let second = ControlPlane::new(store.clone());
    let (first_result, second_result) = tokio::join!(
        first.claim_task(ClaimTask {
            node_id: NodeId::parse("node_first")?,
            lease_token: LeaseToken::parse("lease_first")?,
            now: UnixMillis::new(4_100)?,
            lease_duration: LeaseDuration::new(1_000)?,
        }),
        second.claim_task(ClaimTask {
            node_id: NodeId::parse("node_second")?,
            lease_token: LeaseToken::parse("lease_second")?,
            now: UnixMillis::new(4_100)?,
            lease_duration: LeaseDuration::new(1_000)?,
        })
    );

    let winning_lease = match (first_result, second_result) {
        (Ok(lease), Err(StoreError::NoTaskAvailable))
        | (Err(StoreError::NoTaskAvailable), Ok(lease)) => lease,
        other => return Err(format!("unexpected claim outcomes: {other:?}").into()),
    };
    assert_eq!(winning_lease.request, request);
    assert_eq!(winning_lease.expires_at, UnixMillis::new(5_100)?);

    let mut retry_value = distinct_request_value("retry_02")?;
    retry_value["evaluation_id"] = request.evaluation_id().as_str().into();
    retry_value["attempt_number"] = 2_u64.into();
    let retry_request = decode_evaluation_request(&serde_json::to_vec(&retry_value)?)?;
    let control = ControlPlane::new(store);
    assert_eq!(
        control
            .retry_expired(RetryExpired {
                request: retry_request.clone(),
                now: UnixMillis::new(5_100)?,
            })
            .await,
        Err(StoreError::LeaseConflict)
    );

    let snapshot = control
        .retry_expired(RetryExpired {
            request: retry_request.clone(),
            now: UnixMillis::new(5_101)?,
        })
        .await?;
    assert_eq!(snapshot.state, EvaluationState::Queued);
    assert_eq!(
        snapshot.current_attempt_id,
        retry_request.attempt_id().clone()
    );
    assert_eq!(snapshot.attempt_number, 2);
    assert_eq!(snapshot.attempt_state, AttemptState::Queued);
    assert_eq!(
        control
            .retry_expired(RetryExpired {
                request: retry_request.clone(),
                now: UnixMillis::new(5_102)?,
            })
            .await?,
        snapshot
    );

    let history: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT attempt_id, attempt_number, state FROM evaluation_attempts \
         WHERE evaluation_id = $1 ORDER BY attempt_number",
    )
    .bind(request.evaluation_id().as_str())
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        history,
        [
            ("attempt_01".to_owned(), 1, "expired".to_owned()),
            ("attempt_retry_02".to_owned(), 2, "queued".to_owned()),
        ]
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn judge_claim_dispatches_only_capability_compatible_tasks(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool);
    store.migrate().await?;
    let algorithm_request = decode_evaluation_request(VALID_REQUEST)?;
    let mut network_value = distinct_request_value("network_only")?;
    network_value["required_capabilities"] = serde_json::json!(["network"]);
    let network_request = decode_evaluation_request(&serde_json::to_vec(&network_value)?)?;
    let control = ControlPlane::new(store.clone());
    control
        .create_evaluation(CreateEvaluation {
            request: algorithm_request.clone(),
            created_at: UnixMillis::new(10_000)?,
        })
        .await?;
    control
        .create_evaluation(CreateEvaluation {
            request: network_request,
            created_at: UnixMillis::new(10_001)?,
        })
        .await?;

    let command = JudgeClaim {
        node_id: NodeId::parse("judge_node_01")?,
        declared_capabilities: vec![Capability::parse("algorithm.batch")?],
        operation_id: ClaimOperationId::parse("claim_01")?,
        lease_token: LeaseToken::parse("lease_01")?,
        now: UnixMillis::new(11_000)?,
        lease_policy: LeasePolicy::new(LeaseDuration::new(30_000)?, LeaseDuration::new(10_000)?)?,
    };
    let lease = store.judge_claim_task(command.clone()).await?;

    assert_eq!(
        lease.request.evaluation_id(),
        algorithm_request.evaluation_id()
    );
    assert_eq!(lease.node_id.as_str(), "judge_node_01");
    assert_eq!(store.judge_claim_task(command).await?, lease);
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn result_is_lease_fenced_atomic_and_idempotent(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let control = ControlPlane::new(store.clone());
    control
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(6_000)?,
        })
        .await?;
    let lease_token = LeaseToken::parse("lease_result")?;
    control
        .claim_task(ClaimTask {
            node_id: NodeId::parse("node_result")?,
            lease_token: lease_token.clone(),
            now: UnixMillis::new(6_100)?,
            lease_duration: LeaseDuration::new(1_000)?,
        })
        .await?;
    let result = openoj_application::evaluate(
        &request,
        &mut DecisionExecutor {
            decision: Decision::new(Verdict::Accepted, Score::new(1, 1)?)?,
        },
    )?;
    let command = SubmitResult {
        idempotency_key: IdempotencyKey::parse("result_key_01")?,
        lease_token,
        result,
        now: UnixMillis::new(6_500)?,
    };

    let completed = control.submit_result(command.clone()).await?;
    let replayed = control.submit_result(command.clone()).await?;
    assert_eq!(completed, replayed);
    assert_eq!(completed.state, EvaluationState::Completed);
    assert_eq!(completed.attempt_state, AttemptState::Completed);
    assert!(completed.terminal_result);

    let different_result = openoj_application::evaluate(
        &request,
        &mut DecisionExecutor {
            decision: Decision::new(Verdict::WrongAnswer, Score::new(0, 1)?)?,
        },
    )?;
    assert_eq!(
        control
            .submit_result(SubmitResult {
                result: different_result,
                ..command
            })
            .await,
        Err(StoreError::IdempotencyConflict)
    );

    let stored_results: i64 =
        sqlx::query_scalar("SELECT count(*) FROM evaluations WHERE terminal_result IS NOT NULL")
            .fetch_one(&pool)
            .await?;
    assert_eq!(stored_results, 1);
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn result_rejects_the_wrong_lease_token(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool);
    store.migrate().await?;
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let control = ControlPlane::new(store);
    control
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(7_000)?,
        })
        .await?;
    control
        .claim_task(ClaimTask {
            node_id: NodeId::parse("node_fence")?,
            lease_token: LeaseToken::parse("lease_correct")?,
            now: UnixMillis::new(7_100)?,
            lease_duration: LeaseDuration::new(1_000)?,
        })
        .await?;
    let result = openoj_application::evaluate(
        &request,
        &mut DecisionExecutor {
            decision: Decision::new(Verdict::Accepted, Score::new(1, 1)?)?,
        },
    )?;

    assert_eq!(
        control
            .submit_result(SubmitResult {
                idempotency_key: IdempotencyKey::parse("result_wrong_lease")?,
                lease_token: LeaseToken::parse("lease_wrong")?,
                result,
                now: UnixMillis::new(7_500)?,
            })
            .await,
        Err(StoreError::StaleLease)
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn result_rejects_an_expired_lease(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool);
    store.migrate().await?;
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let control = ControlPlane::new(store);
    control
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(8_000)?,
        })
        .await?;
    let lease_token = LeaseToken::parse("lease_expiring")?;
    control
        .claim_task(ClaimTask {
            node_id: NodeId::parse("node_expiring")?,
            lease_token: lease_token.clone(),
            now: UnixMillis::new(8_100)?,
            lease_duration: LeaseDuration::new(1_000)?,
        })
        .await?;
    let result = openoj_application::evaluate(
        &request,
        &mut DecisionExecutor {
            decision: Decision::new(Verdict::Accepted, Score::new(1, 1)?)?,
        },
    )?;

    assert_eq!(
        control
            .submit_result(SubmitResult {
                idempotency_key: IdempotencyKey::parse("result_expired_lease")?,
                lease_token,
                result,
                now: UnixMillis::new(9_101)?,
            })
            .await,
        Err(StoreError::StaleLease)
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn queued_cancellation_is_atomic_and_idempotent(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let control = ControlPlane::new(store);
    control
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(10_000)?,
        })
        .await?;
    let result = openoj_application::evaluate(&request, &mut CancellationExecutor)?;
    let command = CancelEvaluation {
        idempotency_key: IdempotencyKey::parse("cancel_key_01")?,
        evaluation_id: request.evaluation_id().clone(),
        result,
        now: UnixMillis::new(10_100)?,
    };

    let cancelled = control.cancel_evaluation(command.clone()).await?;
    let replayed = control.cancel_evaluation(command).await?;
    assert_eq!(cancelled, replayed);
    assert_eq!(cancelled.state, EvaluationState::Cancelled);
    assert_eq!(cancelled.attempt_state, AttemptState::Cancelled);
    assert!(cancelled.terminal_result);
    let task_state: String =
        sqlx::query_scalar("SELECT state FROM evaluation_tasks WHERE attempt_id = $1")
            .bind(request.attempt_id().as_str())
            .fetch_one(&pool)
            .await?;
    assert_eq!(task_state, "cancelled");
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn cancellation_and_completion_race_keeps_one_terminal_result(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    store.migrate().await?;
    let request = decode_evaluation_request(VALID_REQUEST)?;
    let setup = ControlPlane::new(store.clone());
    setup
        .create_evaluation(CreateEvaluation {
            request: request.clone(),
            created_at: UnixMillis::new(11_000)?,
        })
        .await?;
    let lease_token = LeaseToken::parse("lease_race")?;
    setup
        .claim_task(ClaimTask {
            node_id: NodeId::parse("node_race")?,
            lease_token: lease_token.clone(),
            now: UnixMillis::new(11_100)?,
            lease_duration: LeaseDuration::new(1_000)?,
        })
        .await?;
    let completed_result = openoj_application::evaluate(
        &request,
        &mut DecisionExecutor {
            decision: Decision::new(Verdict::Accepted, Score::new(1, 1)?)?,
        },
    )?;
    let cancelled_result = openoj_application::evaluate(&request, &mut CancellationExecutor)?;
    let result_control = ControlPlane::new(store.clone());
    let cancel_control = ControlPlane::new(store);

    let (result_outcome, cancel_outcome) = tokio::join!(
        result_control.submit_result(SubmitResult {
            idempotency_key: IdempotencyKey::parse("result_race")?,
            lease_token,
            result: completed_result,
            now: UnixMillis::new(11_500)?,
        }),
        cancel_control.cancel_evaluation(CancelEvaluation {
            idempotency_key: IdempotencyKey::parse("cancel_race")?,
            evaluation_id: request.evaluation_id().clone(),
            result: cancelled_result,
            now: UnixMillis::new(11_500)?,
        })
    );

    let winner = match (result_outcome, cancel_outcome) {
        (Ok(snapshot), Err(StoreError::TerminalConflict))
        | (Err(StoreError::TerminalConflict), Ok(snapshot)) => snapshot,
        other => return Err(format!("unexpected terminal race outcomes: {other:?}").into()),
    };
    assert!(matches!(
        winner.state,
        EvaluationState::Completed | EvaluationState::Cancelled
    ));
    let terminal_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM evaluations \
         WHERE evaluation_id = $1 AND terminal_result IS NOT NULL",
    )
    .bind(request.evaluation_id().as_str())
    .fetch_one(&pool)
    .await?;
    assert_eq!(terminal_rows, 1);
    Ok(())
}
