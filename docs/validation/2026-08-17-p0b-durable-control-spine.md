---
status: Validated
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0-B durable control spine commits c574a0c through 84869f8 plus synchronized CI and documentation
references:
  - README.md
  - ../requirements/acceptance.md
  - ../security/threat-model.md
  - ../architecture/overview.md
  - ../operations/postgresql.md
  - ../superpowers/specs/2026-08-17-p0b-durable-control-spine-design.md
---

# P0-B 持久化控制脊柱验证记录（2026-08-17）

## 范围与环境

- 范围：有界时间/租约和生命周期、application storage port、PostgreSQL v1 migration、Evaluation/Attempt/可靠任务事务、租约领取、显式过期恢复、幂等结果/取消以及最小 CLI。
- 功能修订：`c574a0cdf784f21b980fa2d4e066cb43782a7bb5` 至 `84869f844a9f3623712aff2448132e8758d30c6c`；本记录及 CI/运维同步作为其后的证据提交，不改变公开协议。
- 环境：Arch Linux，Linux `7.1.8-zen1-3-zen`，x86_64；Rust/Cargo 1.97.1、cargo-deny 0.20.2。
- 数据库：本机 disposable PostgreSQL 17.10 container，loopback `55432`，数据目录位于 tmpfs；测试 harness 为每个 `#[sqlx::test]` 创建隔离数据库。
- 构建：debug workspace build/test；不包含 release 性能构建。
- Firecracker/kernel/rootfs/guest agent/Runtime image：未实现、未运行、无摘要。
- 输入：仓库自有 canonical 最小 JSON fixture，无真实用户源码、秘密或受限制题目。
- 原始摘要：`artifacts/2026-08-17-p0b-durable-control-spine.txt`，SHA-256 `335a900db063d858026a0a104aff839ce5df70a9e0fa86397d59a57382095d57`。

## 需求与控制追踪

| 范围 | 实现边界 | 直接证据 | 当前结论 |
|---|---|---|---|
| `FR-SUBMISSION-001` / `ACC-P0-012` | Evaluation、Attempt、Problem Version、Submission、Runtime 不同身份与不可变引用 | `create_is_atomic_and_same_payload_replay_is_idempotent`、`every_immutable_reference_rejects_metadata_drift` | P0-B control/storage slice 已实现；尚无真实执行 |
| `FR-SCHED-001` / `ACC-P0-003` | 单项 task、竞争领取、租约 fencing、过期后新 Attempt 并保留历史 | `concurrent_claim_has_one_winner_and_expiry_creates_attempt_two` | 数据库崩溃恢复语义已实现；真实进程强杀未验证 |
| `FR-RESULT-001` / `CTL-IDEMPOTENCY-001` / `ACC-P0-018` | 创建、retry、结果、取消写边界幂等与原子终态 | storage 11 项集成测试中的重放、冲突、rollback 和竞态用例 | PostgreSQL transaction 边界已验证 |
| `CTL-STATE-001` / `ACC-P0-017` | 终态不可逆、取消/完成 first-terminal-wins | `terminal_evaluation_state_cannot_transition`、`cancellation_and_completion_race_keeps_one_terminal_result` | 控制平面状态竞态已验证；停止 microVM 未实现 |
| `ACC-P0-016` | 请求/结果、ID、时间、租约、单项 claim、CLI 文件与连接池有硬上限 | domain/protocol 边界测试、`bounded_reader_rejects_oversized_input_before_decode`、`database_pool_size_is_bounded`、DB CHECK | P0-B 边界已实现；执行资源尚无证明 |
| `ACC-P0-001` | CLI migrate/submit/status 到持久化数据库 | CLI 集成测试与真实二进制进程冒烟 | 仅 early control-plane slice；不构成端到端判题闭环 |

## 自动与手工检查

以下命令在上述环境 fresh run 通过：

```text
bash scripts/check-docs.sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
DATABASE_URL=<redacted-loopback-test-url> cargo test --workspace --all-targets --locked
python3 scripts/check-workspace.py
cargo deny --locked check advisories bans sources
git diff --check
```

共 36 项测试通过：application 6、CLI 4、domain 10、protocol 5、PostgreSQL storage 11；0 failed、0 ignored。storage 测试使用真实 PostgreSQL transaction 和行锁，不 mock 数据库边界。

真实二进制进程依次执行 `openoj-cli migrate`、对 canonical fixture 执行 `submit`、对 `eval_01` 执行 `status`。migration 返回 schema 1；submit 与 status 都返回 `eval_01`、`attempt_01`、attempt 1、queued、无终态结果。连接串未写入原始摘要。

`cargo deny` 的 advisory、ban 和 source 子门禁通过，同时报告 SQLx 传递图存在多个重复版本；这是可见的供应链/体积残余风险，不是当前配置下的门禁失败。完整 license gate 仍按仓库许可证决策保持阻塞。

## 安全、兼容与运维结论

- 控制平面不执行用户进程；P0-B 没有加入宿主执行回退、guest 凭证或网络能力。
- 请求/结果在写入和读取时经过 canonical 编解码；损坏持久化请求返回稳定错误而不是 panic。
- claim 使用单项行锁和 lease token；结果校验 Attempt、node、token、expiry 与当前身份，过期/错误 token 被拒绝。
- migration 可重复执行，schema 高于 1 时 fail closed；已提交 migration 未被后续功能提交改写。
- CLI 参数、输入、连接池均有硬边界，错误输出不包含连接串、SQL 或 canonical payload。
- 运维契约要求显式 migrate-before-start、可恢复备份、保留数据的应用回滚和单独审批的数据修复。

## 未验证与阻塞

- GitHub Actions 的 PostgreSQL 18 service 尚需远端运行后才能形成该版本证据；本地只验证 PostgreSQL 17.10。
- KVM/Firecracker、judge node、guest、真实编译/运行/check、对象存储、HTTP、认证和停止运行中 microVM 均未实现。
- 未执行真实进程强杀、主机重启、网络分区、复制/故障转移、备份恢复演练或跨版本滚动升级。
- 未运行 release benchmark，不声明吞吐、延迟、容量、长期增长或高可用。
- 平台许可证、依赖允许列表、DCO/CLA 和正式分发权利仍等待人工决策；不得正式发布或分发产物。
