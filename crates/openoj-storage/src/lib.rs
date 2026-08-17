mod create;
mod lease;
mod migration;
mod model;
mod read;
mod terminal;

pub use migration::SUPPORTED_SCHEMA_VERSION;

use openoj_application::StoreError;
use openoj_application::{
    CancelEvaluation, ClaimTask, CreateEvaluation, EvaluationSnapshot, EvaluationStore, JudgeClaim,
    JudgeRenew, JudgeRenewDirective, RetryExpired, StoreFuture, SubmitResult, TaskLease,
};
use openoj_domain::EvaluationId;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub const MAX_DATABASE_POOL_SIZE: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabasePoolSize(u32);

impl DatabasePoolSize {
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        if value >= 1 && value <= MAX_DATABASE_POOL_SIZE {
            Some(Self(value))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

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
    /// Atomically claims one ready task whose requirements are a subset of a judge node's
    /// declared capabilities.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] for unavailable, corrupt, or unavailable task state.
    pub async fn judge_claim_task(&self, command: JudgeClaim) -> Result<TaskLease, StoreError> {
        lease::judge_claim_task(&self.pool, command).await
    }

    /// Extends or cancels a current P0-C lease using only control-plane time and policy.
    ///
    /// # Errors
    ///
    /// Returns a stable [`StoreError`] for stale ownership, terminal state, or persistence failure.
    pub async fn judge_renew_lease(
        &self,
        command: JudgeRenew,
    ) -> Result<JudgeRenewDirective, StoreError> {
        lease::judge_renew_lease(&self.pool, command).await
    }

    /// Connects to `PostgreSQL` with an already validated bounded pool size.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Unavailable`] without exposing the connection string or `SQLx` source.
    pub async fn connect(
        database_url: &str,
        pool_size: DatabasePoolSize,
    ) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_size.value())
            .connect(database_url)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(Self::from_pool(pool))
    }

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
