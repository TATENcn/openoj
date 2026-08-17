mod migration;

pub use migration::SUPPORTED_SCHEMA_VERSION;

use openoj_application::StoreError;
use sqlx::PgPool;

#[derive(Clone)]
pub struct PostgresEvaluationStore {
    pool: PgPool,
}

impl PostgresEvaluationStore {
    #[must_use]
    pub const fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Applies all embedded, forward-only `OpenOJ` database migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Unavailable`] when `PostgreSQL` cannot apply or verify a migration.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        migration::run(&self.pool).await
    }

    /// Refuses a database whose `OpenOJ` schema is missing, corrupt, or newer than this binary.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::IncompatibleSchema`] for every unsupported schema version and
    /// [`StoreError::Unavailable`] when `PostgreSQL` cannot be reached.
    pub async fn check_compatibility(&self) -> Result<(), StoreError> {
        migration::check_compatibility(&self.pool).await
    }
}
