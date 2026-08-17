---
status: Proposed
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0-C implementation execution
references:
  - ../specs/2026-08-17-p0c-judge-control-design.md
  - ../../architecture/decisions/0004-grpc-over-uds-for-p0-judge-control.md
  - ../../requirements/functional.md
  - ../../requirements/non-functional.md
  - ../../requirements/acceptance.md
---

# P0-C Judge Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一个仅通过 Unix Domain Socket 通信的 P0-C Judge Control 最小闭环：控制平面可按 capability 安全地派发 Evaluation，独立 Judge Node 使用显式 development mock executor 完成领取、续租、取消与结果提交。

**Architecture:** 保持 domain、application 与 storage 的分层。`openoj-judge-protocol` 从唯一 `.proto` wire contract 生成 Tonic binding 并只做 transport/canonical JSON 转换；`openoj-judge-core` 持有单并发、transport-neutral worker 状态机；两个 app crate 只组装 UDS、配置、生命周期与适配器。控制平面拥有时钟、租约策略、token 和数据库事务，judge node 不接触 SQLx 或数据库凭证。

**Tech Stack:** Rust 1.97.1、Tokio、SQLx/PostgreSQL 18、Protobuf/Prost/Tonic over UDS、vendored protoc、现有 canonical JSON Schema protocol。

## Global Constraints

- Scope is `FR-SCHED-001`, `NFR-REL-001`, `NFR-OPEN-001`, their P0 acceptance criteria, and `ADR-0004`; P0-C excludes Firecracker, KVM, jailer, user code, artifact bodies, TCP/mTLS, public HTTP and network execution.
- `.proto` is the only Judge Control wire source; canonical `EvaluationRequest` and `EvaluationResult` stay JSON bytes and are validated at conversion boundaries.
- The production path never executes untrusted user code on the host. P0-C development mock must be explicit and production configuration must reject it.
- New database state uses an additive v2 forward migration only. Existing migration v1 is immutable.
- Every production behavior starts with a focused failing test; generated bindings and static configuration are exempt only where a behavior test exercises them.
- New direct dependencies are exact-pinned, feature-minimized, documented, lockfile-reviewed and cargo-deny checked.

## File Map

```text
schemas/openoj/judge-control/v0alpha1/judge-control.proto       # canonical internal RPC contract
crates/openoj-judge-protocol/                                  # generated binding + bounded conversion
crates/openoj-domain/                                          # shared node/lease value invariants only
crates/openoj-application/                                    # server-owned judge-control use cases and ports
crates/openoj-storage/migrations/*_p0c_judge_control.sql       # v2 forward schema/backfill
crates/openoj-storage/src/                                    # transactional claim/renew/result adapter
crates/openoj-judge-core/                                     # one-lease worker state machine + development mock
apps/openoj-control-plane/                                    # PostgreSQL + UDS Tonic server assembly
apps/openoj-judge-node/                                       # UDS client + typed node configuration assembly
tests/ or app tests/                                          # child-process PostgreSQL + UDS regression coverage
docs/architecture/, docs/security/, docs/operations/, docs/validation/ # synchronized facts/evidence
```

---

### Task 1: Create the bounded v0alpha1 Judge Control protocol crate

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `scripts/check-workspace.py`, `docs/architecture/workspace-and-crates.md`, `docs/architecture/technology-stack.md`
- Create: `schemas/openoj/judge-control/v0alpha1/judge-control.proto`
- Create: `crates/openoj-judge-protocol/Cargo.toml`, `crates/openoj-judge-protocol/build.rs`, `crates/openoj-judge-protocol/src/lib.rs`, `crates/openoj-judge-protocol/tests/protocol_contract.rs`

- [ ] Write tests that assert exact v0alpha1 negotiation/version acceptance, duplicate-or-over-64 capability rejection, request/response byte limits, malformed canonical JSON rejection, and descriptor generation.
- [ ] Run the focused protocol test and observe each new contract test fail because the crate or service is absent.
- [ ] Add the `.proto` service with unary `Negotiate`, `Claim`, `RenewLease`, and `SubmitResult`; generate bindings into `OUT_DIR` using vendored protoc, with no checked-in generated copy.
- [ ] Implement bounded conversion helpers and transport-neutral error categories; preserve canonical request/result validation and reject unsupported/unknown inputs fail-closed.
- [ ] Add exact-pinned workspace dependencies, update dependency-direction checks and architecture/technology facts, then run the focused test green.

### Task 2: Add server-owned Judge Control application semantics

**Files:**
- Modify: `crates/openoj-domain/src/**/*.rs`, `crates/openoj-application/src/**/*.rs`, related unit tests

- [ ] Write application tests proving node allowlist defaults to deny, client-supplied time and lease duration cannot override server policy, and valid lease policy produces bounded renew scheduling.
- [ ] Run the focused application tests and observe the missing command/port behavior fail.
- [ ] Introduce typed node context, allowlist/capability policy, server clock, lease-token source, claim/renew/result commands, directives, snapshots and stable errors without Tonic/SQLx types.
- [ ] Map existing Evaluation state machine and canonical result semantics through explicit application ports; retain P0-B CLI behavior.
- [ ] Run focused domain/application tests green and format only touched Rust files.

### Task 3: Migrate and enforce capability-aware claim persistence

**Files:**
- Create: `crates/openoj-storage/migrations/*_p0c_judge_control.sql`
- Modify: `crates/openoj-storage/src/**/*.rs`, `crates/openoj-storage/tests/**/*.rs`, `docs/operations/database.md` (or canonical successor)

- [ ] Write PostgreSQL tests for empty v1-to-v2 migration, repeat migration, strict request-payload capability backfill, malformed backfill rollback, capability subset dispatch, and schema-v2 startup refusal rules.
- [ ] Run these storage tests against PostgreSQL 18 and observe failure before adding migration/adapter behavior.
- [ ] Add only forward schema changes for sorted unique task capabilities, claim operation metadata, and indexes; fail the migration atomically on invalid historical payloads.
- [ ] Implement transaction-safe capability matching and service-owned lease-token persistence; preserve `FOR UPDATE SKIP LOCKED` and existing first-terminal-wins semantics.
- [ ] Run focused storage migration/dispatch tests green.

### Task 4: Implement claim replay, renew and cancellation transactions

**Files:**
- Modify: `crates/openoj-storage/src/**/*.rs`, `crates/openoj-storage/tests/**/*.rs`, `crates/openoj-application/src/**/*.rs`

- [ ] Write tests for same node + same claim operation returning the same lease, semantic drift/different node conflict, stale token/attempt/node rejection, renew expiry extension, and leased cancellation returning `cancel` without revival.
- [ ] Observe the focused tests fail on the existing P0-B store implementation.
- [ ] Add atomic claim-replay, renew-directive and result-idempotency operations to the application/store port and SQLx adapter; use server time and policy only.
- [ ] Add concurrency coverage showing two competing nodes cannot receive two valid leases for one task and a cancel/result race has one terminal winner.
- [ ] Run focused storage/application tests green.

### Task 5: Build the transport-neutral one-worker Judge Core

**Files:**
- Create: `crates/openoj-judge-core/Cargo.toml`, `crates/openoj-judge-core/src/lib.rs`, `crates/openoj-judge-core/src/worker.rs`, `crates/openoj-judge-core/src/mock.rs`, `crates/openoj-judge-core/tests/worker.rs`
- Modify: `Cargo.toml`, `scripts/check-workspace.py`, `docs/architecture/workspace-and-crates.md`

- [ ] Write fake-control-client/fake-executor tests for no-task clamp, response-loss claim replay, retry budget, one concurrent lease, at-least-one renew, stale lease, cancel-before-success, result replay, and graceful shutdown.
- [ ] Run the worker tests and observe failure because `openoj-judge-core` does not exist.
- [ ] Implement a bounded state machine and traits independent of Tonic, SQLx and Firecracker; ensure a lease cannot create more than one executor future or submit operation.
- [ ] Implement deterministic development mock execution without shell, child-process, filesystem-content or artifact-body execution; mark output `development_mock` and `production_eligible = false`.
- [ ] Run worker tests green and update the workspace boundary checker/documentation.

### Task 6: Assemble typed UDS server and node clients

**Files:**
- Create: `apps/openoj-control-plane/Cargo.toml`, `apps/openoj-control-plane/src/main.rs`, `apps/openoj-control-plane/src/config.rs`, `apps/openoj-control-plane/src/server.rs`
- Create: `apps/openoj-judge-node/Cargo.toml`, `apps/openoj-judge-node/src/main.rs`, `apps/openoj-judge-node/src/config.rs`, `apps/openoj-judge-node/src/client.rs`
- Modify: root workspace configuration and app-focused tests

- [ ] Write configuration and UDS tests for relative/symlink/non-directory socket parents, unsafe existing paths, unknown/duplicate node configuration, production-plus-mock rejection, and a valid 0700-parent/0600-socket startup.
- [ ] Run focused tests and observe the absent binaries/configuration fail.
- [ ] Implement strict typed configuration, protected UDS listener/client connection, deadline and bounded retry enforcement, and mapping between gRPC status and stable application errors.
- [ ] Assemble control plane with schema v2 check and allowlist; assemble judge node with explicit development mock only. Do not instantiate a TCP listener.
- [ ] Implement shutdown behavior: cease claiming, signal executor cancellation, wait at most five seconds, and never fabricate a result.
- [ ] Run app-focused tests green.

### Task 7: Prove the real P0-C process boundary and failure paths

**Files:**
- Create/Modify: process integration test harness and fixtures under the selected app or workspace test directory
- Modify: `docs/operations/`, `docs/security/`, `docs/architecture/`, `docs/validation/`

- [ ] Write an integration test that starts PostgreSQL 18, migrates v2, launches both binaries on a temporary UDS directory, submits with the CLI, and waits for one terminal development-mock result.
- [ ] Observe the test fail before wiring the real process transport.
- [ ] Wire the smallest production code necessary for process test success; avoid test-only backdoors or in-process server substitution.
- [ ] Add tests for disconnected socket, judge-node restart, duplicate result, stale lease and capability mismatch; assert no duplicate terminal state and no sensitive payload/token in returned gRPC errors.
- [ ] Run the process suite green with the disposable PostgreSQL 18 instance and record exact environment/limits as evidence, not a production-security conclusion.

### Task 8: Complete repository gates and evidence

**Files:**
- Modify: relevant CI workflow(s), `docs/validation/*`, requirement-to-test mapping and affected architecture/security/operations facts

- [ ] Add or update CI to run proto/descriptor reproducibility, protocol and process tests where the runner supports PostgreSQL, workspace direction checks and dependency policy without requiring a system protoc.
- [ ] Update facts to describe only implemented P0-C behavior; keep Firecracker, KVM, TCP/mTLS, cross-host identity, performance and production isolation explicitly `Unverified`.
- [ ] Run `cargo fmt --check`, workspace build, Clippy with `-D warnings`, all tests against PostgreSQL 18, dependency policy, workspace/document checks and every OpenOJ skill quick validation.
- [ ] Inspect final diff and requirement trace; record commands, results, residual risks, environment limits and rollback behavior before a reviewable commit.
