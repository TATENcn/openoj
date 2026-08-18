---
status: Validated
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0-D expired-lease recovery slice (agent/p0d-expired-recovery)
references:
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../architecture/overview.md
  - ../architecture/decisions/0002-modular-control-and-execution-planes.md
  - ../superpowers/plans/2026-08-17-p0d-expired-lease-recovery.md
---

# P0-D 过期租约恢复验证记录（2026-08-17）

## 范围与环境

- 范围：把 P0-B/P0-C 已实现的 `retry_expired` 存储原语接入 control-plane 进程路径，新增后台 sweeper，周期性扫描并重入队过期 `leased` 租约；为新 attempt 生成新 `AttemptId` 与 `IdempotencyKey`；新增 lease/recovery 配置。关闭 P0-C 验证记录中"真实进程强杀后的端到端 claim replay 未验证"的缺口。
- 环境：Arch Linux，x86_64；Rust/Cargo 1.97.1；debug 构建。
- 数据库：disposable PostgreSQL 18.6 container，loopback `127.0.0.1:55433`；连接串只经测试环境变量传入。
- 进程边界：真实 `openoj-control-plane`（含 sweeper）、`openoj-judge-node` 与 `openoj-cli` 子进程。

## 需求与证据

| 范围 | 直接证据 | 当前结论 |
|---|---|---|
| `FR-SCHED-001` / `ACC-P0-003` | `expired_lease_is_recovered_and_completed_by_a_new_judge_node`（真实子进程）、`recover_expired_re_enqueues_an_expired_lease_with_a_new_attempt`、`concurrent_recovery_recovers_an_expired_lease_exactly_once` | 过期 leased 被 sweeper 重入队为新 queued attempt（attempt_number+1、新 attempt/idempotency key），新节点可重领完成终态；并发恢复单行单一获胜，Attempt 历史保留。 |
| `ACC-P0-018` / `CTL-IDEMPOTENCY-001` | 恢复写路径复用 `retry_expired` 的原子事务与幂等/身份校验；`with_next_attempt_advances_identity_and_preserves_semantics` | 恢复为独立的幂等操作，不重复计分、不覆盖历史。 |
| `ACC-P0-017` / `CTL-STATE-001` | `recover_expired` 只处理 `leased`+过期状态；终态/未过期租约跳过 | 状态转换确定，不复活已终态或未过期任务。 |
| `ADR-0002` | sweeper 位于 control-plane（控制平面拥有可靠 task/outbox 与恢复）；judge-node 不接触数据库 | 所有权边界保持。 |

## 已执行检查

以下命令在 PostgreSQL 18.6 disposable container（loopback `127.0.0.1:55433`）fresh run 全部通过，共 **81 项测试、0 失败、0 忽略**：

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

新增回归覆盖：
- `openoj-domain`：`with_next_attempt` 推进 attempt 身份与编号、更换 idempotency key、保留其余语义。
- `openoj-storage`：`recover_expired` 扫描+重入队、并发恢复单一获胜、Attempt 历史保留。
- 并发修复：并发 `recover_expired` 竞争同一租约时，后到的 `retry_expired` 可能因 PostgreSQL 对旧 JOIN 行的重评估返回 `NotFound`；`recover_expired` 将其与状态冲突同等视为"已被处理"并跳过，确保单行单一获胜。
- `openoj-control-plane` 进程级：真实 sweeper 恢复过期租约并被新节点完成终态。

## 配置新增（control-plane env，均有界）

- `OPENOJ_LEASE_DURATION_MS`：默认 `30000`，范围 `1..=3_600_000`。
- `OPENOJ_RENEW_AFTER_MS`：默认 `10000`，范围 `1..<lease/2`（`LeasePolicy` 校验）。
- `OPENOJ_RECOVERY_INTERVAL_MS`：默认 `5000`，范围 `250..=60_000`。

## 未验证

- Firecracker/KVM/jailer、guest、用户代码、Artifact 内容体、网络、TCP/mTLS、跨主机节点身份、性能与生产隔离均未实现或未验证。
- 断线重连、运行中取消停止路径、五秒排空、runtime image 回收仍未做进程级回归。
- GitHub Actions 远端 PostgreSQL 18 job 独立结果与维护者审核仍是本记录 `Validated` 的前提。
- 本记录不声明性能、容量、高可用、跨版本滚动升级或生产隔离结论。
