use std::error::Error;
use std::fs;

use openoj_cli::{
    CliCommand, CliError, execute_with_store, parse_args, parse_database_pool_size, read_bounded,
};
use openoj_domain::UnixMillis;
use openoj_protocol::MAX_EVALUATION_REQUEST_BYTES;
use openoj_storage::PostgresEvaluationStore;
use sqlx::PgPool;

#[test]
fn parser_accepts_only_the_three_bounded_commands() {
    assert_eq!(parse_args(["migrate"]), Ok(CliCommand::Migrate));
    assert!(matches!(
        parse_args(["submit", "request.json"]),
        Ok(CliCommand::Submit { .. })
    ));
    assert!(matches!(
        parse_args(["status", "eval_01"]),
        Ok(CliCommand::Status { .. })
    ));
    assert_eq!(parse_args(["submit"]), Err(CliError::Usage));
    assert_eq!(parse_args(["migrate", "unexpected"]), Err(CliError::Usage));
    assert!(matches!(
        parse_args(["status", "-bad"]),
        Err(CliError::InvalidEvaluationId)
    ));
}

#[test]
fn database_pool_size_is_bounded() {
    assert_eq!(
        parse_database_pool_size(None).map(openoj_storage::DatabasePoolSize::value),
        Ok(4)
    );
    assert_eq!(
        parse_database_pool_size(Some("1")).map(openoj_storage::DatabasePoolSize::value),
        Ok(1)
    );
    assert_eq!(
        parse_database_pool_size(Some("64")).map(openoj_storage::DatabasePoolSize::value),
        Ok(64)
    );
    for invalid in ["0", "65", "not-a-number"] {
        assert_eq!(
            parse_database_pool_size(Some(invalid)),
            Err(CliError::InvalidDatabasePoolSize)
        );
    }
}

#[test]
fn bounded_reader_rejects_oversized_input_before_decode() -> Result<(), Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "openoj-cli-bounded-{}-{}.json",
        std::process::id(),
        MAX_EVALUATION_REQUEST_BYTES
    ));
    fs::write(&path, vec![b'x'; MAX_EVALUATION_REQUEST_BYTES + 1])?;
    let result = read_bounded(&path, MAX_EVALUATION_REQUEST_BYTES);
    fs::remove_file(path)?;

    assert_eq!(result, Err(CliError::InputTooLarge));
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn migrate_submit_replay_and_status_use_the_durable_store(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let store = PostgresEvaluationStore::from_pool(pool.clone());
    assert_eq!(
        execute_with_store(CliCommand::Migrate, store.clone(), UnixMillis::new(12_000)?).await?,
        "migrated schema 2"
    );

    let path = std::env::temp_dir().join(format!(
        "openoj-cli-submit-{}-{}.json",
        std::process::id(),
        12_000
    ));
    fs::write(
        &path,
        include_bytes!("../../../schemas/openoj/v0alpha1/fixtures/evaluation-request.valid.json"),
    )?;
    let expected =
        "evaluation=eval_01 attempt=attempt_01 attempt_number=1 state=queued terminal_result=false";
    let first = execute_with_store(
        CliCommand::Submit { path: path.clone() },
        store.clone(),
        UnixMillis::new(12_001)?,
    )
    .await?;
    let replay = execute_with_store(
        CliCommand::Submit { path },
        store.clone(),
        UnixMillis::new(12_002)?,
    )
    .await?;
    assert_eq!(first, expected);
    assert_eq!(replay, expected);
    assert_eq!(
        execute_with_store(
            CliCommand::Status {
                evaluation_id: openoj_domain::EvaluationId::parse("eval_01")?,
            },
            store,
            UnixMillis::new(12_003)?,
        )
        .await?,
        expected
    );
    fs::remove_file(std::env::temp_dir().join(format!(
        "openoj-cli-submit-{}-{}.json",
        std::process::id(),
        12_000
    )))?;

    let evaluations: i64 = sqlx::query_scalar("SELECT count(*) FROM evaluations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(evaluations, 1);
    Ok(())
}
