---
name: verify-openoj
description: Independently verify or audit OpenOJ changes, pull requests, implementations, documentation, security controls, compatibility, and release evidence against repository requirements and gates. Use when reviewing work, checking completion, validating a claim, investigating whether a change is safe to merge, or producing an evidence-backed gap report; do not use it to silently implement missing product behavior or weaken failing checks.
---

# Verify OpenOJ

Verify from authoritative requirements and raw evidence rather than trusting the change description.

## Load the verification baseline

1. Read `AGENTS.md`, `CONTRIBUTING.md`, and `docs/README.md`.
2. Read every requirement, acceptance item, threat, risk, ADR, protocol, and operations fact claimed by the change.
3. Read `docs/development/testing.md`, `docs/development/workflow.md`, and `docs/validation/README.md`.
4. Load a specialized Skill when verification touches protocol or sandbox-specific rules.

## Freeze the review scope

1. Record the requested claim and target revision/worktree.
2. Inspect Git status, branch, diff, staged diff, and relevant history without mutating them.
3. Separate requested changes, unrelated user changes, generated changes, and pre-existing failures.
4. Identify missing references or acceptance criteria before running tests.

Do not stage, commit, rebase, rewrite, fix, or delete anything unless the user separately authorizes implementation.

## Trace requirements to implementation

For each claimed ID:

1. Locate the implementation boundary.
2. Locate positive, rejection, boundary, cancellation, timeout, cleanup, and resource-exhaustion tests that apply.
3. Confirm documentation status matches actual implementation and validation.
4. Confirm protocol producers/consumers, migrations, generated code, and operational behavior were updated together.
5. Confirm no out-of-scope behavior or privilege was added.

Treat a missing trace as a gap, not as implicit success.

## Execute gates

1. Run the narrowest tests that reproduce the behavior.
2. Run applicable additional verification from `docs/development/workflow.md`.
3. Run workspace format, build, lint, test, dependency, and documentation gates when available.
4. Use release builds and recorded environments for performance claims.
5. Use real supported KVM/Firecracker environments for isolation claims; otherwise mark them Unverified.
6. Preserve raw command output or a stable artifact reference for non-trivial conclusions.

Do not turn environmental inability into a pass. Distinguish Failed, Blocked, Not Run, and Passed.

## Audit security and compatibility

1. Re-evaluate affected `THR-*` and `CTL-*` entries.
2. Check default-deny behavior, data minimization, bounded resources, audit, and host/other-task survival.
3. Check unknown fields/values, version mismatch, rolling combinations, idempotency, duplicate delivery, and rollback.
4. Search the diff for secret exposure, raw user data, high-cardinality metrics, shell interpolation, broad capabilities, `unsafe`, panic paths, and unbounded queues.
5. Confirm tests do not mock away the boundary being claimed.

## Report findings

Lead with whether the requested claim is supported. Then report:

- actionable findings ordered by severity, each with a precise file/line and violated fact;
- passed requirements and their evidence;
- commands, environments, and raw result summaries;
- unverified or blocked claims;
- unrelated/pre-existing failures;
- residual risk and required human approvals.

Do not praise or summarize before listing correctness, security, compatibility, or data-loss findings. If there are no findings, state that explicitly without claiming absence of all defects.

## Stop conditions

Stop and report when the target revision changes during verification, required evidence would need destructive/production action, a secret or active exploit is discovered, or authorization is insufficient. Preserve confidentiality and follow `SECURITY.md` for vulnerability details.
