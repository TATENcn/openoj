---
status: Validated
owners: OpenOJ control-plane, storage, operations and security maintainers
last_reviewed: 2026-08-26
applies_to: development operator cancellation command at commit 60bdbe73dde3c390f0756bc5665c2c2567d50d6f
references:
  - ../architecture/overview.md
  - ../architecture/workspace-and-crates.md
  - ../operations/algorithm-c-development-runtime.md
  - ../operations/postgresql.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../security/threat-model.md
  - ../security/trust-boundaries.md
  - 2026-08-26-inflight-microvm-cancellation.md
---

# Development operator 取消命令验证（2026-08-26）

## 范围与结论

本记录验证 commit `60bdbe73dde3c390f0756bc5665c2c2567d50d6f`：受信 development
operator 可执行 `openoj-cli cancel <evaluation-id> <caller-stable-idempotency-key>`，通过真实
CLI 进程、PostgreSQL、control-plane UDS、judge-node 和 Attempt-owned Firecracker 句柄取消正在
运行的 microVM。相同目标和幂等键的重放返回相同终态；8 秒无限循环 Run workload 被中断，
Firecracker API/vsock 路径在 3 秒内消失，judge 进程保持存活。

取消边界不再接受调用者构造的 `EvaluationResult`。storage 在锁住当前 Evaluation/Attempt 的同一
transaction 内解码已持久化 canonical request，由 application 生成唯一取消结果，再执行
first-terminal-wins 写入。CLI 无法注入 Verdict、Score、Attempt、provenance、stage、usage、
diagnostic 或 evidence。这为 `FR-JUDGE-003`、`FR-SCHED-001`、`FR-RESULT-001`、
`ACC-P0-003/017/018` 与 `CTL-IDEMPOTENCY-001` 提供 development-only 局部证据。

## 信任、身份与幂等边界

CLI 只接受一个有界 `EvaluationId` 和一个有界、由调用者在同一意图重试时稳定复用的
`IdempotencyKey`；进程时间转换为有界 `UnixMillis`。参数数量、非法 Unicode、格式和长度在进入
storage 前拒绝，storage 错误经稳定 `CliError::Storage` 脱敏。

storage 使用 `SELECT ... FOR UPDATE` 锁住当前 Evaluation 与 Attempt，从该 Attempt 的
`request_payload` 生成取消结果，并核对 Evaluation/Attempt identity。结果使用
`DevelopmentMock`、`production_eligible=false`、无 node provenance，明确表示控制面决策，不伪装
为 guest、Firecracker 或 judge node 生成的执行证据。相同 key/payload 返回相同快照；不同 key、
不同终态或结果提交竞态不能覆盖既有终态。

本命令直接使用 deployment 注入的 PostgreSQL 凭证，只是 development 运维入口。它不含主体认证、
授权 scope、审批、限流或结构化审计，不得向不可信用户暴露，也不满足 production 管理能力的
`ACC-P0-011/019` 完整边界。

## 环境与产物

- Arch Linux x86_64，kernel `7.1.8-zen1-3-zen`；AMD Ryzen 9 9950X3D，8 个可见 CPU，
  15 GiB 内存；VMware 暴露 AMD-V。
- `/dev/kvm` 为 `crw-rw-rw- root:kvm`，KVM API `12`；Firecracker `1.16.1`；Rust/Cargo
  `1.97.1`。
- 一次性 PostgreSQL 18 Alpine 绑定本机随机端口，测试后删除；image digest 为
  `postgres@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2`。

| 产物 | SHA-256 |
|---|---|
| kernel | `882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef` |
| rootfs | `4f4121a3f5ea7dc6e34e63f3d704b77f3da30aebb3d7d55f846b13badff3461c` |
| guest agent | `e775d7d0fe71b8980a58ad02b2a9b8ac1195d6b32fc745db659750e46373cae2` |
| runtime manifest | `c89e99d95ed258901bda15d00c9154e956ab1295d3f4043d771394ed5461a23b` |
| SPDX SBOM | `aee7b475b81add5b58f497accdad5d4ef10a5ba8ad06e89007e2427e904ecddb` |

## 测试与结果

以下门禁通过：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
DATABASE_URL=<PostgreSQL 18> OPENOJ_TEST_DATABASE_URL=<same> OPENOJ_REQUIRE_KVM=1 OPENOJ_FC_TEST_IMAGES=<runtime-out> cargo test --workspace --all-targets --locked
OPENOJ_REQUIRE_KVM=1 OPENOJ_TEST_DATABASE_URL=<PostgreSQL 18> OPENOJ_FC_TEST_IMAGES=<runtime-out> cargo test -p openoj-control-plane --test process_e2e real_microvm_cancellation_interrupts_and_reclaims --locked -- --nocapture --test-threads=1
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
# change-openoj-architecture、develop-openoj、verify-openoj 的 quick validator
```

完整 workspace 共 157 个测试通过、0 失败；其中 9 个 process E2E 在 17.41 秒通过，严格 runtime
KVM smoke 在 1.73 秒通过，17 个 PostgreSQL control-spine 测试在 1.74 秒通过。聚焦真实 KVM CLI
取消 case 在 10.34 秒完成，其中 microVM 回收仍满足小于 3 秒的独立断言。CLI integration 4 个测试
在 0.42 秒通过，覆盖参数、首次取消、相同 key 重放、status 和不同 key 终态冲突。`cargo deny`
只有既有 duplicate-version 警告；测试后无 Firecracker 进程残留。

精简原始证据位于 `artifacts/2026-08-26-operator-cancel-command.txt`，SHA-256 为
`ea6ceea4b53bf100a18b786813c01e040a997c721465867f2eb5e7ba0e80202c`。

## 兼容、回退与未验证项

canonical schema、生成绑定、host/guest `v0alpha1`、数据库 schema、migration、runtime image、网络、
设备和凭证格式均未变化。变更只调整未发布的内部 Rust `CancelEvaluation` command：移除 caller
result payload。回退代码和本记录即可恢复旧接口，无持久化数据迁移。

以下仍为 `Unverified`：

- production 取消 API 的身份认证、授权、主体、scope、审批、限流和结构化审计；
- stage-accurate 取消位置；当前控制面 canonical result 因未持久化 live stage progress，只能把计划
  的首个 stage 标记为 cancelled，不能声称它记录了真实中断 stage；
- 真实 KVM 下租约丢失与 control UDS 断连触发的中断；
- guest 在 workload 运行中消费 cooperative `Cancel`；
- production jailer/cgroup/seccomp/watchdog 与资源耗尽矩阵。
