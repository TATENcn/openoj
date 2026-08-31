---
status: Validated
owners: OpenOJ control-plane, execution-plane, operations and security maintainers
last_reviewed: 2026-08-26
applies_to: in-flight real-microVM interruption at commit 89698e67316a13b8a0a1ea45e7592906a3297f29
references:
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../architecture/overview.md
  - ../architecture/workspace-and-crates.md
  - ../operations/algorithm-c-development-runtime.md
  - ../protocol/guest-vsock-v0alpha1.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../security/threat-model.md
  - ../security/trust-boundaries.md
  - 2026-08-25-algorithm-c-cancellation-race.md
---

# 真实 microVM 运行中取消验证（2026-08-26）

## 范围与结论

本记录验证 commit `89698e67316a13b8a0a1ea45e7592906a3297f29`：固定无限循环 C Artifact
经 CLI、PostgreSQL、control-plane UDS、judge-node、Firecracker/vsock 和 guest agent 进入真实
microVM 后，控制面取消会触发 Attempt 专属 Firecracker VMM 句柄。worker 在等待同步 executor 的
同时按协商周期续租；收到 `Cancel`、陈旧租约或续租错误时请求终止，等待 teardown 后丢弃执行
结果；提交前的最后一次续租关闭“执行完成到提交”的取消竞态。

真实 KVM 回归使用 8 秒 Run wall-time 预算，但取消后不再等待该 deadline：Firecracker API socket
与 vsock 路径在 3 秒断言窗口内消失，judge-node 保持运行，持久化终态仍为 `Cancelled`。这为
`FR-JUDGE-003`、`FR-SCHED-001`、`ACC-P0-003/017/018`、`THR-RESOURCE-001`、
`CTL-RECLAIM-001`、`CTL-IDEMPOTENCY-001` 与 `RISK-EXEC-001` 提供 development-only 局部证据。

## 架构与安全边界

取消能力遵循最小权限：`FirecrackerVmCancellation` 只持有其 Attempt 创建的一个 child 句柄，只能
幂等地请求 `start_kill`，不能枚举或终止其他进程。Firecracker 生命周期仍由 executor teardown
统一 `wait` 并删除 API/vsock 路径；原子取消标记覆盖“取消先于 child 注册”的竞态。judge worker
通过 `spawn_blocking` 承载同步 executor，不把 microVM ownership 移入控制平面，也不允许控制平面
直接启动或管理用户进程。

本变更实现 Accepted ADR-0005 的现有 execution-plane ownership，不改变 crate 依赖方向、信任边界、
设备、网络、凭证、数据库 migration 或 runtime image。host 与 guest 协议版本仍为 `v0alpha1`，
canonical schema 和生成绑定均未变化。已有 guest `Cancel/Cancelled` 消息语义保持不变；当前 guest
agent 在同步等待 child 时不能及时消费新消息，因此即时安全回收使用 host-owned VMM 句柄，而没有
宣称 guest cooperative cancellation 已验证。

## 编排与断言

测试为每次运行创建独立数据库、私有 control UDS 和全新 microVM。请求源码摘要为
`84950edbf9514ebef845181f9f296bf2a71be3032f0d1d27b28b93fe1fd57f54`，Run wall-time
预算为 8 秒，control-plane 协商续租周期为 250 毫秒。测试观察 Firecracker API socket 后提交
canonical cancellation，并要求：

- 持久化状态保持 `state=cancelled terminal_result=true`，迟到结果不能覆盖终态；
- Attempt-owned Firecracker child 被中断，不等待 8 秒 guest stage deadline；
- API socket 与 vsock 路径在 3 秒内消失；
- judge-node 在普通取消后仍存活并继续 control loop；
- CLI status 保持可响应。

确定性单元测试另覆盖 blocking execution 收到 `Cancel`、续租返回陈旧租约、执行完成后的最终续租
fence，以及 Firecracker 精确 child ownership、重复取消和 pre-spawn 取消竞态。

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

## 命令与结果

以下命令通过：

```text
OPENOJ_REQUIRE_KVM=1 OPENOJ_TEST_DATABASE_URL=<PostgreSQL 18> OPENOJ_FC_TEST_IMAGES=<runtime-out> cargo test -p openoj-control-plane --test process_e2e real_microvm_cancellation_interrupts_and_reclaims --locked -- --nocapture --test-threads=1
OPENOJ_REQUIRE_KVM=1 OPENOJ_TEST_DATABASE_URL=<PostgreSQL 18> OPENOJ_FC_TEST_IMAGES=<runtime-out> cargo test -p openoj-control-plane --test process_e2e real_microvm_ --locked -- --nocapture --test-threads=1
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
DATABASE_URL=<PostgreSQL 18> OPENOJ_TEST_DATABASE_URL=<same> OPENOJ_REQUIRE_KVM=1 OPENOJ_FC_TEST_IMAGES=<runtime-out> cargo test --workspace --all-targets --locked
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
# change-openoj-architecture、evolve-openoj-protocol、change-openoj-sandbox、develop-openoj、verify-openoj 的 quick validator
```

4 个真实 microVM process case 在 17.73 秒通过。完整 workspace 共 156 个测试通过、0 失败；其中
9 个 process E2E 在 12.45 秒通过，严格 runtime KVM smoke 在 1.77 秒通过，17 个 PostgreSQL
control-spine 测试在 2.00 秒通过。`cargo deny` 只有既有 duplicate-version 警告。测试后无
Firecracker 进程残留；生成的 runtime outputs 未进入 Git。

精简原始证据位于 `artifacts/2026-08-26-inflight-microvm-cancellation.txt`，SHA-256 为
`cc1fa67b2a9ef449f1ed3d0f3c5c6e521bb4943266aaea206733975101c043fe`。

## 回退与未验证项

回退实现 commit 和本记录即可恢复上一 PR 的 deadline-bounded cancellation fencing；没有 migration、
schema 或 Artifact 需要逆向迁移。以下仍为 `Unverified`：

- 真实 KVM 下由租约丢失或 control UDS 断连触发的中断；陈旧租约当前只有确定性 worker 单元测试；
- control UDS 重连行为及断连到回收的精确 deadline；
- guest 运行 workload 时消费 cooperative `Cancel`；
- production jailer/cgroup/seccomp/watchdog profile；
- CPU、内存、process、disk、I/O、无限输出与并发 guest 的资源耗尽矩阵；
- 面向 operator 的 API/CLI 取消命令；本测试直接调用现有 application/storage 取消路径；
- release build 下经统计采样的取消延迟与性能结论。
