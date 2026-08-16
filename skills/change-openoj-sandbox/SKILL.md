---
name: change-openoj-sandbox
description: Design, implement, harden, or verify OpenOJ Firecracker execution isolation, jailer configuration, KVM and host setup, guest agent, vsock, kernel/rootfs, block devices, networking, snapshots, caches, cgroups, seccomp, watchdogs, resource measurement, teardown, and malicious-workload defenses. Use for any change that can alter untrusted-code privileges, host attack surface, cross-task state, resource limits, execution provenance, or production sandbox claims; apply this low-freedom workflow before coding or benchmarking such changes.
---

# Change OpenOJ Sandbox

Preserve defense in depth and require target-environment evidence for every isolation claim.

## Establish authority and environment

1. Read `AGENTS.md`, `CONTRIBUTING.md`, `docs/governance/change-control.md`, and the relevant requirements.
2. Read `docs/security/threat-model.md`, `trust-boundaries.md`, `docs/architecture/decisions/0001-rust-and-firecracker.md`, architecture, deployment, testing, and validation facts.
3. Inspect the exact host kernel, KVM, Firecracker, jailer, guest kernel/rootfs, guest agent, cgroup, uid/gid, network, storage, and watchdog configuration.
4. Mark mock, container-only, nested-virtualization, or missing-KVM environments as non-production evidence.
5. Identify operations requiring elevated privileges or production access; obtain explicit user authorization before performing them.

Do not run destructive host cleanup, change production networking, mount unknown images, start untrusted workloads outside the approved sandbox, or create releases implicitly.

## Map threats and invariants

1. Reference affected `THR-*`, `CTL-*`, `NFR-SEC-*`, and `ACC-*` IDs.
2. Describe attacker-controlled bytes, host resources touched, trust transitions, secrets, shared state, and failure blast radius.
3. Preserve these invariants unless an Accepted ADR explicitly changes them:
   - control plane never directly executes the workload;
   - guest receives no platform long-term secret;
   - guest network is absent by default;
   - every resource and output path is bounded;
   - guest output is untrusted and cannot self-declare final success;
   - teardown is idempotent and cross-task state is cleared;
   - unsupported or mismatched configurations fail closed.
4. Require a Proposed ADR and human security approval for any relaxed invariant, new device, network path, host capability, shared cache, snapshot reuse, or privilege.

## Design before implementation

1. Specify Firecracker/jailer API calls, ownership, uid/gid, cgroup, namespace, seccomp, file descriptors, block/vsock devices, and lifecycle state machine.
2. Define CPU, memory, process, wall time, disk, I/O, output, log, queue, network, and retry limits with overflow behavior.
3. Define cancellation, VMM unresponsiveness, guest crash, host restart, partial setup, result race, teardown, and reconciler behavior.
4. Define immutable image sources, digests, signatures, SBOM, cache keys, snapshot hygiene, and architecture compatibility.
5. Define observability that is useful without exposing guest content or creating unbounded logs/labels.

## Implement narrowly

1. Keep platform and privileged code in the designated Firecracker/system adapter.
2. Minimize `unsafe`, privilege duration, device surface, writable host paths, and guest-visible metadata.
3. Parse bounded versioned vsock messages; do not provide a generic shell.
4. Build commands from validated executable IDs and argument arrays, never shell interpolation.
5. Make setup and teardown transactional or reconciliable after every partial failure.
6. Keep mock executor behavior visibly distinct from production and impossible to select accidentally in production configuration.

## Run adversarial verification

Test at least:

- CPU loop, memory pressure, process/thread explosion, disk fill, I/O flood and infinite output;
- guest hang, panic, reboot, abrupt vsock close, malformed/oversized message and stage timeout;
- invalid kernel/rootfs/runtime digest, unsupported architecture and version mismatch;
- cancellation races before start, during execution and during result submission;
- repeated teardown, VMM deadlock/watchdog kill and host restart reconciliation;
- snapshot/cache cross-task data leakage and sensitive-log redaction;
- host responsiveness, concurrent unrelated task survival and bounded resource recovery.

Use release builds and record the complete environment. Do not claim security, fairness, startup latency, density, or cleanup timing from mocks or missing-KVM CI.

## Deliver and review

Update threat, architecture, operations, runtime, test, risk, and validation facts in the same change. Report changed attack surface, controls, refused alternatives, exact environment and commands, evidence, residual risks, Unverified targets, rollback, and required human approval. Finish with `verify-openoj`.

## Stop conditions

Stop on unexpected host impact, secret exposure, suspected escape, cross-task data leakage, uncontrolled resource growth, ambiguous cleanup, unsupported production configuration, or missing authority. Preserve evidence safely, avoid public exploit details, and follow `SECURITY.md`.
