---
status: Validated
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0-C Judge Control commits 582d499 through the P0-C failure-path closeout (agent/p0c-closeout-failure-paths)
references:
  - README.md
  - ../requirements/functional.md
  - ../requirements/acceptance.md
  - ../architecture/decisions/0004-grpc-over-uds-for-p0-judge-control.md
  - ../operations/deployment-profiles.md
  - ../security/trust-boundaries.md
  - ../superpowers/specs/2026-08-17-p0c-judge-control-design.md
---

# P0-C Judge Control 验证记录（2026-08-17）

## 范围与环境

- 范围：`v0alpha1` Judge Control Protobuf/gRPC 契约、server-owned node policy/clock/lease token、capability claim、renew/cancel、lease-fenced canonical result submit、仅 UDS 的 control-plane/judge-node，以及显式 development mock。
- 环境：Arch Linux，x86_64；Rust/Cargo 1.97.1；debug 构建。
- 数据库：disposable PostgreSQL 18，loopback `127.0.0.1:55432`；连接串只经测试环境变量传入，未写入本记录或测试输出。
- 进程边界：真实 `openoj-control-plane`、`openoj-judge-node` 和 `openoj-cli` 子进程，使用临时 `0700` 目录与 `0600` UDS；不以 in-process transport 替代。

## 需求与证据

| 范围 | 直接证据 | 当前结论 |
|---|---|---|
| `FR-SCHED-001` / `ACC-P0-003` | `postgres_uds_and_two_child_processes_reach_a_terminal_mock_result`；storage 的 capability/claim replay/renew 集成测试 | PostgreSQL、UDS 与独立进程完成 claim、renew、result 持久化。真实进程强杀后的租约重领依赖 `retry_expired`，该原语未接入任何进程循环，故端到端强杀恢复仍未验证。 |
| `FR-JUDGE-003` / `ACC-P0-017` | `async_worker_checks_renewal_before_submitting`、`async_worker_does_not_fabricate_a_result_after_cancel`、`async_worker_propagates_stale_lease_without_fabricating_a_result` | worker 执行前检查 renew；cancel 与 stale lease 都不伪造结果。运行中 microVM 停止未实现。 |
| `FR-RESULT-001` / `ACC-P0-018` | canonical result schema-to-domain conversion、storage result replay 测试、真实进程终态检查 | node/result 的 identity、provenance、lease token 和 operation replay 在服务端 fence。 |
| `NFR-OPEN-001` / `ADR-0004` | `uds_server_negotiates_without_a_tcp_listener`、版本/allowlist 回归、`judge_node_without_a_control_listener_fails_closed`、`unknown_identity_judge_node_fails_closed`、`unmatched_capability_judge_node_fails_closed_and_leaves_task_queued` | P0-C 只创建 UDS listener；未知版本/节点/capability 与缺失 listener 均 fail closed，capability 不匹配的节点无法让任务到达终态。 |
| `FR-JUDGE-001` 早期边界 | worker 失败路径单测：`worker_returns_no_task_without_executing_or_submitting`、`worker_propagates_claim_error_without_executing`、`worker_propagates_execution_error_without_submitting`、`worker_propagates_submit_error_after_execution`、`async_worker_returns_no_task_without_submitting`、`async_worker_propagates_submit_error_after_renewal` | 传输无关 worker 状态机在 no-task、claim/execute/submit 错误与 stale lease 下都短路，不提交未完成结果。 |

## 已执行检查

以下命令在 PostgreSQL 18.6 disposable container（loopback `127.0.0.1:55432`）fresh run 全部通过，共 **77 项测试、0 失败、0 忽略**：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
DATABASE_URL=<redacted-loopback-test-url> OPENOJ_TEST_DATABASE_URL=<redacted> cargo test --workspace --all-targets --locked
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
cargo deny --locked check advisories bans sources
git diff --check
```

新增回归覆盖（本收尾）：
- `crates/openoj-judge-core/tests/worker_failures.rs`：7 项传输无关 worker 失败路径单测（no-task、claim/execute/submit 错误、stale lease 不伪造结果）。
- `apps/openoj-control-plane/tests/process_e2e.rs`：新增 3 项真实子进程 fail-closed 测试（缺失 listener、未知身份、capability 不匹配且任务保持 queued）。每个进程测试使用独立 disposable 数据库（`FreshDatabase`）并在结束时 `DROP ... WITH (FORCE)`，避免与 `#[sqlx::test]` 并行竞争和残留。

UDS 与 PostgreSQL 测试需要允许创建 Unix socket 并访问 disposable loopback database 的环境；受限文件系统沙箱不提供该能力。GitHub Actions 的 PostgreSQL 18 job 运行 workspace test，因此包含该 process regression；远端运行结果仍需在此记录确认。

## 未验证

- Firecracker/KVM/jailer、guest、用户代码、Artifact 内容体、网络、TCP/mTLS、跨主机节点身份、性能与生产隔离均未实现或未验证。
- 真实进程强杀后的端到端 claim replay：`retry_expired` 作为存储原语已验证（P0-B），但未接入任何 control-plane/judge-node 进程循环，因此进程级强杀恢复仍需专门的 sweeper/回收路径与回归后才能声明。
- 断线后的 deadline/重连、运行中取消的停止路径、五秒排空、runtime image 回收仍未做进程级回归。
- GitHub Actions 远端 PostgreSQL 18 job 的独立结果与维护者审核仍是本记录 `Validated` 的前提。
- 本记录不声明性能、容量、高可用、跨版本滚动升级或生产隔离结论。
