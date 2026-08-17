use std::error::Error;

use openoj_application::StoreError;
use openoj_storage::PostgresEvaluationStore;
use sqlx::PgPool;

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
    assert_eq!(schema_version, 1);
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

    sqlx::query("UPDATE openoj_schema_metadata SET schema_version = 2 WHERE singleton")
        .execute(&pool)
        .await?;
    assert_eq!(
        store.check_compatibility().await,
        Err(StoreError::IncompatibleSchema)
    );
    Ok(())
}
