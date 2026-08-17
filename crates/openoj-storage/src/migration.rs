use std::sync::LazyLock;

use openoj_application::StoreError;
use sqlx::PgPool;
use sqlx::SqlSafeStr;
use sqlx::migrate::{Migration, MigrationType, Migrator};

pub const SUPPORTED_SCHEMA_VERSION: i32 = 1;

static MIGRATOR: LazyLock<Migrator> = LazyLock::new(|| {
    Migrator::with_migrations(vec![Migration::new(
        202_608_170_001,
        "p0b control spine".into(),
        MigrationType::Simple,
        include_str!("../migrations/202608170001_p0b_control_spine.sql").into_sql_str(),
        false,
    )])
});

pub async fn run(pool: &PgPool) -> Result<(), StoreError> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;
    check_compatibility(pool).await
}

pub async fn check_compatibility(pool: &PgPool) -> Result<(), StoreError> {
    let version = sqlx::query_scalar::<_, i32>(
        "SELECT schema_version FROM openoj_schema_metadata WHERE singleton",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| StoreError::Unavailable)?;

    if version == Some(SUPPORTED_SCHEMA_VERSION) {
        Ok(())
    } else {
        Err(StoreError::IncompatibleSchema)
    }
}
