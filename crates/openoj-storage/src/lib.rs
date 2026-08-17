mod create;
mod lease;
mod migration;
mod model;
mod read;
mod terminal;

pub use migration::SUPPORTED_SCHEMA_VERSION;

use openoj_application::StoreError;
use openoj_application::{
    CancelEvaluation, ClaimTask, CreateEvaluation, EvaluationSnapshot, EvaluationStore,
    RetryExpired, StoreFuture, SubmitResult, TaskLease,
};
use openoj_domain::EvaluationId;
use sqlx::PgPool;

#[derive(Clone)]
pub struct PostgresEvaluationStore {
    pool: PgPool,
}

impl EvaluationStore for PostgresEvaluationStore {
    fn create_evaluation(&self, command: CreateEvaluation) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(create::create_evaluation(&self.pool, command))
    }

    fn evaluation_status(
        &self,
        evaluation_id: EvaluationId,
    ) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(read::evaluation_status(&self.pool, evaluation_id))
    }

    fn claim_task(&self, command: ClaimTask) -> StoreFuture<'_, TaskLease> {
        Box::pin(lease::claim_task(&self.pool, command))
    }

    fn retry_expired(&self, command: RetryExpired) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(lease::retry_expired(&self.pool, command))
    }

    fn submit_result(&self, command: SubmitResult) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(terminal::submit_result(&self.pool, command))
    }

    fn cancel_evaluation(&self, command: CancelEvaluation) -> StoreFuture<'_, EvaluationSnapshot> {
        Box::pin(terminal::cancel_evaluation(&self.pool, command))
    }
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
