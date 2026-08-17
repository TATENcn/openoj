---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0-B durable control spine implementation
references:
  - ../specs/2026-08-17-p0b-durable-control-spine-design.md
  - ../../requirements/acceptance.md
  - ../../architecture/workspace-and-crates.md
  - ../../development/testing.md
---

# P0-B Durable Control Spine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a PostgreSQL-backed, bounded and idempotent Evaluation/Attempt/task/result control path with a minimal CLI and real database recovery tests.

**Architecture:** Pure lifecycle and bounded time types remain in `openoj-domain`; `openoj-application` owns commands, snapshots, stable errors and the async storage port; `openoj-storage` implements the port with SQLx transactions and embedded forward migrations. The CLI composes these layers without exposing database or protocol representation inside domain types.

**Tech Stack:** Rust 2024/MSRV 1.97, PostgreSQL, SQLx 0.9.0, Tokio 1.53.1, existing Serde/canonical JSON protocol, GitHub Actions PostgreSQL service.

## Global Constraints

- Preserve `ADR-0002`: modular Rust control plane, PostgreSQL reliable task/outbox and an independent future judge node.
- Do not change `schemas/openoj/v0alpha1/open-evaluation.schema.json` or public protocol semantics.
- Persist at most 262,144 request bytes and 1,048,576 result bytes; never persist Artifact content in scheduling rows.
- Accept Unix milliseconds only in `0..=253_402_300_799_999` and lease durations only in `1..=3_600_000`.
- Keep every queue operation single-item and the CLI pool in `1..=64` connections, default 4.
- Use forward-only migrations; never edit a migration after its first shared commit.
- Every production behavior follows RED → observed expected failure → GREEN → refactor.
- Commit bodies include `Refs:`, `Tests:` and `Unverified:` and never include `Co-authored-by:`.

---

### Task 1: Domain lifecycle, time and lease values

**Files:**
- Create: `crates/openoj-domain/src/control.rs`
- Modify: `crates/openoj-domain/src/value.rs`
- Modify: `crates/openoj-domain/src/lib.rs`

**Interfaces:**
- Consumes: existing `DomainError` and bounded opaque-ID parser.
- Produces: `LeaseToken`, `UnixMillis`, `LeaseDuration`, `EvaluationState`, `AttemptState`, `MAX_UNIX_MILLIS`, `MAX_LEASE_DURATION_MS`, and checked state transitions.

- [ ] **Step 1: Write failing domain tests**

```rust
#[test]
fn terminal_evaluation_state_cannot_transition() {
    assert_eq!(
        EvaluationState::Completed.transition_to(EvaluationState::Queued),
        Err(DomainError::InvalidTransition {
            entity: "evaluation",
            from: "completed",
            to: "queued",
        })
    );
}

#[test]
fn lease_expiry_is_checked_and_bounded() -> Result<(), DomainError> {
    let now = UnixMillis::new(MAX_UNIX_MILLIS)?;
    let duration = LeaseDuration::new(1)?;
    assert!(matches!(now.checked_add(duration), Err(DomainError::OutOfRange { .. })));
    Ok(())
}
```

- [ ] **Step 2: Run `cargo test -p openoj-domain control -- --nocapture` and confirm failures name missing lifecycle/time APIs.**
- [ ] **Step 3: Implement bounded constructors, `as_str`, checked addition, and only the transitions in the approved state graph.**
- [ ] **Step 4: Run `cargo test -p openoj-domain` and confirm all domain tests pass without warnings.**
- [ ] **Step 5: Commit the domain slice with refs `FR-SUBMISSION-001`, `ACC-P0-012`, `ACC-P0-017`, `CTL-STATE-001`.**

### Task 2: Application commands and storage port

**Files:**
- Create: `crates/openoj-application/src/control.rs`
- Modify: `crates/openoj-application/src/lib.rs`

**Interfaces:**
- Consumes: `EvaluationRequest`, `EvaluationResult`, domain states/time/lease values and existing distinct IDs.
- Produces: `StoreFuture<'a, T>`, `EvaluationStore`, `ControlPlane<S>`, `CreateEvaluation`, `ClaimTask`, `RetryExpired`, `SubmitResult`, `CancelEvaluation`, `EvaluationSnapshot`, `TaskLease`, `StoreError`.

```rust
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

pub trait EvaluationStore: Send + Sync {
    fn create_evaluation(&self, command: CreateEvaluation)
        -> StoreFuture<'_, EvaluationSnapshot>;
    fn evaluation_status(&self, evaluation_id: EvaluationId)
        -> StoreFuture<'_, EvaluationSnapshot>;
    fn claim_task(&self, command: ClaimTask) -> StoreFuture<'_, TaskLease>;
    fn retry_expired(&self, command: RetryExpired)
        -> StoreFuture<'_, EvaluationSnapshot>;
    fn submit_result(&self, command: SubmitResult)
        -> StoreFuture<'_, EvaluationSnapshot>;
    fn cancel_evaluation(&self, command: CancelEvaluation)
        -> StoreFuture<'_, EvaluationSnapshot>;
}
```

- [ ] **Step 1: Add a fake in `control.rs` tests and write a failing test proving `ControlPlane::create_evaluation` passes the exact typed command and returns its snapshot without transport/database types.**
- [ ] **Step 2: Run `cargo test -p openoj-application control -- --nocapture`; confirm it fails because the port and use case do not exist.**
- [ ] **Step 3: Implement the focused command/value types, non-exhaustive stable `StoreError`, boxed-future trait and thin generic `ControlPlane<S>`.**
- [ ] **Step 4: Add and run tests for all six use-case methods and `StoreError::Display`; assert errors never contain payload or connection details.**
- [ ] **Step 5: Run `cargo test -p openoj-application` and commit with refs `NFR-REL-001`, `NFR-OPEN-001`, `CTL-IDEMPOTENCY-001`.**

### Task 3: Storage crate, dependency policy and embedded migration

**Files:**
- Create: `crates/openoj-storage/Cargo.toml`
- Create: `crates/openoj-storage/src/lib.rs`
- Create: `crates/openoj-storage/src/migration.rs`
- Create: `crates/openoj-storage/migrations/202608170001_p0b_control_spine.sql`
- Create: `crates/openoj-storage/tests/postgres_control_spine.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `scripts/check-workspace.py`
- Modify: `docs/architecture/workspace-and-crates.md`
- Modify: `docs/development/dependencies.md`

**Interfaces:**
- Consumes: SQLx `PgPool`, embedded `sqlx::migrate!`, application error categories.
- Produces: `PostgresEvaluationStore::connect`, `from_pool`, `migrate`, `check_compatibility`, schema version 1.

- [ ] **Step 1: Add the crate and an integration test that connects through `OPENOJ_TEST_DATABASE_URL`, runs migration on an isolated database/schema, runs it twice, and asserts `schema_version = 1`.**
- [ ] **Step 2: Run the focused integration test with PostgreSQL and confirm RED because migration/store APIs are missing.**
- [ ] **Step 3: Add exact pinned dependencies.**

```toml
sqlx = { version = "=0.9.0", default-features = false, features = ["postgres", "runtime-tokio", "migrate", "tls-rustls-ring-native-roots"] }
tokio = { version = "=1.53.1", default-features = false, features = ["macros", "rt-multi-thread"] }
```

The storage test target additionally enables SQLx `macros` only for `#[sqlx::test]`; production migration embedding uses `include_str!` and `Migrator::with_migrations`.

- [ ] **Step 4: Write the forward migration with immutable reference tables, `evaluations`, `evaluation_attempts`, `evaluation_tasks`, FK/unique constraints, state checks and byte/time/length checks from the design.**
- [ ] **Step 5: Implement embedded migration and compatibility check; mutate the test schema version to 2 and assert `StoreError::IncompatibleSchema`, then restore/drop the isolated test database.**
- [ ] **Step 6: Run migration tests, `python3 scripts/check-workspace.py`, and `bash scripts/check-docs.sh`; commit the buildable storage foundation.**

### Task 4: Atomic create and status read

**Files:**
- Create: `crates/openoj-storage/src/create.rs`
- Create: `crates/openoj-storage/src/read.rs`
- Create: `crates/openoj-storage/src/model.rs`
- Modify: `crates/openoj-storage/src/lib.rs`
- Modify: `crates/openoj-storage/tests/postgres_control_spine.rs`

**Interfaces:**
- Consumes: `CreateEvaluation`, canonical request encoder/decoder and the migrated schema.
- Produces: `EvaluationStore::create_evaluation`, `evaluation_status`, immutable-reference registration and row-to-snapshot validation.

- [ ] **Step 1: Add a failing real-PostgreSQL test: create one request, assert queued snapshot and counts of exactly one Evaluation/Attempt/task, then replay same key/payload and assert counts stay one.**
- [ ] **Step 2: Run the test and confirm RED at the unimplemented store method.**
- [ ] **Step 3: Implement a single SQL transaction that canonical-encodes, registers/compares Artifact/Problem Version/Submission/Runtime, and inserts Evaluation/Attempt/task.**
- [ ] **Step 4: Add RED tests for same key/different payload, different key/same Evaluation ID and immutable Artifact ID/different digest; implement mappings to `IdempotencyConflict`, `IdentityConflict`, `ImmutableReferenceConflict`.**
- [ ] **Step 5: Add a RED rollback test whose late insert conflicts; assert all domain/task counts remain unchanged, then make transaction rollback pass it.**
- [ ] **Step 6: Decode persisted request on reads and return `CorruptData` on invalid state/payload rather than panicking. Run focused and workspace tests; commit.**

### Task 5: Claim and expired Attempt recovery

**Files:**
- Create: `crates/openoj-storage/src/lease.rs`
- Modify: `crates/openoj-storage/src/lib.rs`
- Modify: `crates/openoj-storage/tests/postgres_control_spine.rs`

**Interfaces:**
- Consumes: `ClaimTask`, `RetryExpired`, domain checked time addition and canonical request comparison.
- Produces: single-item `FOR UPDATE SKIP LOCKED` claim and atomic expired-Attempt replacement.

- [ ] **Step 1: Add a failing concurrency test using two store clones and `tokio::join!`; assert exactly one claim succeeds and the other returns `NoTaskAvailable`.**
- [ ] **Step 2: Implement claim transaction ordered by `(created_at_ms, attempt_id)`, updating task/Attempt/Evaluation and returning a decoded `TaskLease`; run the test green.**
- [ ] **Step 3: Add failing tests proving a live lease cannot be reclaimed and a wrong/overflowing lease duration is rejected before SQL.**
- [ ] **Step 4: Add a failing crash-recovery test: after expiry, submit an otherwise identical request with Attempt 2; assert Attempt 1 is expired, Attempt 2 queued, Evaluation preserved and history count is two.**
- [ ] **Step 5: Implement `retry_expired` with row locks, strict expiry, immutable semantic comparison and consecutive attempt number. Add competition and malformed-retry rejection tests.**
- [ ] **Step 6: Run the storage integration suite and commit with refs `FR-SCHED-001`, `ACC-P0-003`, `ACC-P0-018`.**

### Task 6: Atomic result and cancellation races

**Files:**
- Create: `crates/openoj-storage/src/terminal.rs`
- Modify: `crates/openoj-storage/src/lib.rs`
- Modify: `crates/openoj-storage/tests/postgres_control_spine.rs`

**Interfaces:**
- Consumes: `SubmitResult`, `CancelEvaluation`, canonical result encoder/validator and leased rows.
- Produces: lease-fenced terminal transaction, idempotent result replay and first-terminal-wins cancellation semantics.

- [ ] **Step 1: Add a failing test that claims a task, submits an accepted result, and asserts Evaluation/Attempt/task atomically become completed with one stored result.**
- [ ] **Step 2: Implement identity/status/token/expiry checks and the terminal transaction; run the test green.**
- [ ] **Step 3: Add RED tests for same result key/same bytes replay, same key/different bytes, wrong token, expired lease and stale Attempt; implement stable conflict errors without payload leakage.**
- [ ] **Step 4: Add a RED queued-cancel test with a canonical cancelled result; implement atomic cancellation of Evaluation/Attempt/task.**
- [ ] **Step 5: Add a concurrent cancel/completion test; assert one succeeds, one returns `TerminalConflict`, and exactly one terminal result remains.**
- [ ] **Step 6: Run storage/workspace tests and commit with refs `FR-RESULT-001`, `ACC-P0-017`, `CTL-IDEMPOTENCY-001`, `CTL-STATE-001`.**

### Task 7: Minimal bounded CLI

**Files:**
- Create: `apps/openoj-cli/Cargo.toml`
- Create: `apps/openoj-cli/src/lib.rs`
- Create: `apps/openoj-cli/src/main.rs`
- Modify: `Cargo.toml`
- Modify: `scripts/check-workspace.py`
- Modify: `docs/architecture/workspace-and-crates.md`

**Interfaces:**
- Consumes: `ControlPlane<PostgresEvaluationStore>`, `OPENOJ_DATABASE_URL`, canonical request decoder.
- Produces: `migrate`, `submit <request.json>`, `status <evaluation-id>` and bounded/declassified output.

- [ ] **Step 1: Write failing parser tests for exact accepted argv and rejection of missing/extra arguments.**
- [ ] **Step 2: Write a failing bounded-read test with a sparse/temporary file larger than 262,144 bytes; assert `InputTooLarge` before protocol decode.**
- [ ] **Step 3: Implement focused command enum, bounded reader and stable CLI errors; run unit tests green.**
- [ ] **Step 4: Implement async command runner with pool max 4 and the three approved commands. Never print URL, request/result payload or SQL errors.**
- [ ] **Step 5: Run CLI against the integration PostgreSQL: migrate, submit the canonical fixture twice, and status; assert stable IDs/status and one Evaluation row.**
- [ ] **Step 6: Run workspace direction/build/test/Clippy gates and commit with refs `ACC-P0-001` early slice, `ACC-P0-016`, `ACC-P0-018`.**

### Task 8: CI, operations, validation and final delivery

**Files:**
- Modify: `.github/workflows/rust.yml`
- Modify: `README.md`
- Modify: `docs/architecture/overview.md`
- Modify: `docs/architecture/risks.md`
- Modify: `docs/development/workflow.md`
- Create: `docs/operations/postgresql.md`
- Create: `docs/validation/2026-08-17-p0b-durable-control-spine.md`
- Modify: `docs/validation/README.md`

**Interfaces:**
- Consumes: all implemented commands/tests and existing CI gates.
- Produces: PostgreSQL CI service, operator migration/rollback procedure, acceptance evidence and explicit Unverified list.

- [ ] **Step 1: Add PostgreSQL 18 service healthcheck and `OPENOJ_TEST_DATABASE_URL` to Rust CI; keep merge-group triggers intact.**
- [ ] **Step 2: Document database URL handling, explicit migrate-before-start, schema refusal, pool bound, backup/rollback and non-destructive incident response.**
- [ ] **Step 3: Update architecture/dependency/workflow facts and write validation mapping with exact test names/results; label Firecracker, production PostgreSQL, performance and real process-kill recovery Unverified.**
- [ ] **Step 4: Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --all-targets --locked`, `python3 scripts/check-workspace.py`, `bash scripts/check-docs.sh`, and every repository Skill quick validator that exists.**
- [ ] **Step 5: Inspect `git diff --check`, status, staged/unstaged diff, dependency tree and lockfile for unrelated changes, secrets, debug output or missing docs.**
- [ ] **Step 6: Use `skills/verify-openoj/SKILL.md` and `superpowers:verification-before-completion`; fix every finding through a failing test where behavior changes.**
- [ ] **Step 7: Commit final synchronized docs/CI evidence, push the branch, open a PR to `dev`, wait for checks/review, squash-merge, delete the remote branch, and verify `origin/dev` contains the merge.**
