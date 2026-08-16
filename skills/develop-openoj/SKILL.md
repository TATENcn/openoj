---
name: develop-openoj
description: Implement scoped OpenOJ features, fixes, refactors, tests, documentation, and routine repository changes while preserving requirement, architecture, security, testing, and evidence rules. Use for ordinary implementation work that does not primarily change a public protocol, trust boundary, Firecracker isolation, irreversible architecture decision, destructive migration, or release policy; route those changes through the specialized OpenOJ skill first.
---

# Develop OpenOJ

Implement the smallest complete change that satisfies an identified requirement without weakening repository boundaries.

## Load authoritative sources

1. Read `AGENTS.md`, `CONTRIBUTING.md`, and `docs/README.md`.
2. Read the affected requirement and acceptance IDs in `docs/requirements/`.
3. Read the affected architecture, security, protocol, plugin, development, or operations facts routed by `docs/README.md`.
4. Read all Accepted ADRs referenced by those facts.
5. Switch to the specialized Skill before implementation if the change primarily affects architecture, protocol, or sandbox isolation.

## Establish the task contract

1. State the goal, non-goals, affected components, user-visible result, and stable IDs.
2. Identify security, compatibility, data, plugin, operations, license, and performance impact.
3. List required automated and manual verification.
4. Identify decisions that require human approval under `docs/governance/change-control.md`.
5. Stop and request a decision when a missing choice would materially change public behavior, trust, data, or migration cost.

## Inspect before changing

1. Run the read-only Git checks required by `CONTRIBUTING.md`.
2. Inspect existing code, tests, schemas, migrations, and documentation before proposing new structures.
3. Preserve unrelated working-tree changes and avoid broad formatting.
4. Confirm the target layer from `docs/architecture/workspace-and-crates.md`.

Do not create/switch branches, stage, commit, push, rebase, merge, tag, or publish without explicit user authorization.

## Implement a complete slice

1. Add a failing regression test for a bug or map a new behavior to an acceptance test.
2. Change canonical domain or schema sources before adapters and generated consumers.
3. Keep domain logic independent of transport, database, UI, and VMM types.
4. Implement success, rejection, cancellation, timeout, cleanup, and bounded-resource behavior affected by the change.
5. Use typed errors and structured low-cardinality observability at the responsibility boundary.
6. Update linked facts in the same change; keep future behavior Proposed and real behavior evidence-based.
7. Avoid unrelated refactors, dependency upgrades, generated-file edits, or speculative extension points.

## Verify proportionally

1. Run focused format, compile, lint, and tests for affected components.
2. Run the additional matrix in `docs/development/workflow.md` for the change type.
3. Run workspace gates when the workspace exists.
4. Run `bash scripts/check-docs.sh` for documentation or governance changes.
5. Inspect the final diff for unrelated files, secrets, debug code, untracked TODOs, and unsynchronized facts.
6. Record unavailable KVM, architecture, platform, performance, or external-service checks as Unverified.

Never delete, skip, weaken, or broadly update tests merely to obtain a green result.

## Deliver evidence

Lead with the implemented outcome. Report:

- changed behavior and explicit non-goals;
- referenced requirement, acceptance, risk, and ADR IDs;
- security, compatibility, data, and operations impact;
- exact commands and results;
- manual evidence and environment, if any;
- unverified items and the conditions needed to verify them;
- required human approvals or safe next step.

Do not claim implementation, validation, security, or performance beyond the evidence collected.

## Stop conditions

Stop implementation and report the boundary when:

- no stable requirement or acceptance behavior exists;
- the change silently alters a public protocol or persisted meaning;
- an Accepted ADR or trust boundary must change without approval;
- only one side of a contract can be updated;
- a safe failure, rollback, or cleanup path cannot be defined;
- required work would overwrite unrelated changes or expand authorization;
- verification reveals a pre-existing failure that makes the requested conclusion impossible.
