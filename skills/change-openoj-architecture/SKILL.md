---
name: change-openoj-architecture
description: Design and govern OpenOJ changes to crate responsibilities, dependency direction, process or service topology, data ownership, threading and concurrency models, deployment boundaries, trust zones, storage strategy, or other high-migration-cost technical decisions. Use before implementing cross-cutting architecture changes, introducing foundational infrastructure, splitting or merging services, or changing an Accepted architecture constraint; this skill prepares ADRs and evidence but cannot accept them without authorized human approval.
---

# Change OpenOJ Architecture

Make architecture changes explicit, evidence-backed, reversible where possible, and synchronized with implementation boundaries.

## Load authoritative context

1. Read `docs/product/vision.md`, `docs/product/scope.md`, and `docs/product/glossary.md`.
2. Read affected requirements and acceptance IDs.
3. Read `docs/architecture/overview.md`, `workspace-and-crates.md`, `technology-stack.md`, `risks.md`, and all related ADRs.
4. Read affected security, protocol, operations, dependency, and license facts.
5. Read `docs/governance/change-control.md` and the ADR rules.

## Define the decision

1. State the problem as a required capability or constraint, not as a preferred technology.
2. Record current behavior, data flow, owners, trust boundaries, operational assumptions, and migration limits.
3. Separate decisions already fixed by Accepted requirements from open choices.
4. Decide whether the change needs an Issue, RFC, or ADR.
5. Identify human approval and validation required before implementation.

Stop if the actual product requirement or trust decision is missing.

## Evaluate real alternatives

For at least two viable candidates, compare:

- requirement fit and non-goals;
- trust boundaries and failure blast radius;
- consistency, idempotency, cancellation, and recovery;
- latency, throughput, resource density, and measurement cost;
- protocol, data, deployment, and rolling-upgrade compatibility;
- supply chain, license, maintenance, observability, and removal cost;
- migration, rollback, partial-deployment, and failure behavior.

Do not create a straw-man alternative or choose microservices, queues, caches, plugins, or abstractions without a measured need.

## Record the architecture

1. Create a Proposed ADR using `docs/architecture/decisions/README.md` when required.
2. Update the canonical architecture and risk facts, not only the ADR.
3. Draw the smallest useful data-flow, dependency, state, or trust diagram.
4. Specify invariants, ownership, interfaces, error paths, bounds, and unsupported cases.
5. Define migration phases, compatibility window, rollback trigger, and evidence needed for acceptance.
6. Keep future components Proposed and avoid empty scaffolding.

Only an authorized human maintainer may change the ADR to Accepted.

## Implement after approval

1. Use `develop-openoj` for the scoped implementation.
2. Preserve a buildable state at every shared commit.
3. Add architecture enforcement tests for dependency direction, protocol boundaries, migrations, or configuration where practical.
4. Update security/protocol/operations facts in the same change.
5. Keep compatibility adapters explicit and time-bounded.

## Verify and deliver

Run architecture-relevant gates, failure tests, benchmarks, migration rehearsals, and `verify-openoj`. Report the selected decision, rejected alternatives, consequences, migration/rollback, evidence, residual risks, Unverified items, and required approval.

## Stop conditions

Stop when the decision reduces isolation, risks data loss, breaks public compatibility, changes licensing, introduces a production dependency without source review, or lacks a credible migration/rollback path and the required human approval has not been granted.
