---
name: evolve-openoj-protocol
description: Evolve OpenOJ public evaluation schemas, HTTP representations, events, capabilities, SDK bindings, conformance fixtures, internal RPC contracts, or guest-host messages without silent incompatibility. Use when adding, removing, renaming, retyping, reinterpreting, versioning, or deprecating protocol fields, messages, states, verdicts, errors, artifact metadata, plugin contracts, or producer-consumer behavior; documentation-only wording changes with no semantic effect do not require this skill.
---

# Evolve OpenOJ Protocol

Change canonical semantics first, propagate every boundary, and make compatibility and security behavior executable.

## Load protocol facts

1. Read `docs/product/glossary.md` and affected requirement/acceptance IDs.
2. Read `docs/protocol/README.md`, `docs/protocol/versioning.md`, affected schemas, and related ADRs.
3. Read affected security threats, architecture boundaries, SDK/plugin facts, and migration policies.
4. Inspect existing producers, consumers, generated bindings, fixtures, persisted messages, and version matrices.

## Classify the change

Classify each semantic change as additive compatible, behaviorally compatible, deprecation, alpha-breaking, stable-breaking, or security-critical. Treat changes to units, defaults, requiredness, enum meaning, state transitions, idempotency, ordering, size, authorization, and unknown-value behavior as semantic even if the wire type remains unchanged.

Stop and require a Proposed ADR plus human approval for stable-breaking or security-critical changes.

## Map affected boundaries

List:

- canonical schema and documentation;
- public API and SDKs;
- control-plane and judge-node producers/consumers;
- guest agent and host messages;
- plugin manifests/capabilities;
- stored events, results, fixtures, examples, and conformance implementations;
- rolling-upgrade and downgrade combinations.

Do not begin implementation if only one required side can be changed or tested.

## Implement in order

1. Update the canonical schema and normative semantics.
2. Regenerate bindings using the repository command; never hand-edit generated output.
3. Update explicit schema-to-domain conversion layers.
4. Update every producer and consumer.
5. Add compatibility adapters only with an owner, deprecation condition, and removal plan.
6. Update version/capability negotiation and fail-closed behavior.
7. Update examples, fixtures, SDKs, plugin contracts, and operations matrices.
8. Preserve historical result interpretation and migration provenance.

## Verify

Test:

- schema positive, negative, maximum-size, nesting, unknown enum, unknown field, duplicate and malformed cases;
- old producer/new consumer and new producer/old consumer combinations that are claimed supported;
- unsupported version and capability refusal;
- idempotency, retry, cancellation, state transitions and terminal-state replay;
- generated output reproducibility and no uncommitted generated diff;
- sensitive field redaction and bounded diagnostics;
- conformance fixtures shared across implementations.

Run documentation and workspace gates, then use `verify-openoj` for independent review.

## Deliver

Report semantic changes, compatibility class, supported version matrix, affected producers/consumers, migration/deprecation, exact tests, Unverified combinations, and human approvals. Never describe an alpha contract as stable or infer compatibility from successful compilation alone.

## Stop conditions

Stop when canonical semantics are ambiguous, version ownership is missing, security-critical unknown fields would be ignored, persisted data cannot be interpreted, a rolling combination is claimed without evidence, or compatibility requires silently changing Verdict/Score meaning.
