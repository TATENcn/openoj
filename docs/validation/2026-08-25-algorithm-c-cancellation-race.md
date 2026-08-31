---
status: Validated
owners: OpenOJ control-plane, execution-plane, operations and security maintainers
last_reviewed: 2026-08-25
applies_to: development-only real-microVM cancellation fencing at commit a564d092c738ab38ce04038b61cbb64d204d7b90
references:
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../operations/algorithm-c-development-runtime.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../security/threat-model.md
  - ../security/trust-boundaries.md
  - 2026-08-25-algorithm-c-process-failures.md
---

# algorithm-c 真实 microVM 取消竞态验证（2026-08-25）

## 范围与结论

本记录验证 commit `a564d092c738ab38ce04038b61cbb64d204d7b90`：固定无限循环 C Artifact
经 CLI、PostgreSQL、control-plane UDS、judge-node、Firecracker/vsock 和 guest agent 进入真实
microVM 后，控制面并发提交 canonical cancellation。取消终态以 first-terminal-wins 持久化，重复
取消幂等；judge 完成当前有界 stage 后的迟到结果被拒绝，不能把 `Cancelled` 覆盖成成功或超时；
control-plane status 仍可响应，VMM 与 API/vsock 路径最终回收。

这为 `FR-JUDGE-003`、`FR-SCHED-001`、`ACC-P0-003/017/018`、`THR-RESOURCE-001`、
`CTL-RECLAIM-001`、`CTL-IDEMPOTENCY-001` 与 `RISK-EXEC-001` 提供 development-only 局部证据。
它验证的是终态 fencing 和有界最终回收，不是运行中即时中断。

## 编排与断言

测试为每次运行创建独立数据库、私有 control UDS 和全新 microVM。请求使用源码摘要
`84950edbf9514ebef845181f9f296bf2a71be3032f0d1d27b28b93fe1fd57f54`，Run wall-time
预算为 8 秒。测试在 Firecracker API socket 出现后等待 2 秒，使 guest 有时间完成引导与编译，
然后通过现有 application/storage 取消接口写入终态并立即重放相同命令。

具体断言如下：

- 首次取消和相同 idempotency key/payload 的重放返回相同 `Cancelled` 快照；
- judge-node 在 guest stage 返回后提交迟到结果，因 terminal fence 非零退出；
- CLI status 仍返回 `state=cancelled terminal_result=true`，持久化 Verdict 为 `Cancelled`；
- cancellation result 明确使用 `DevelopmentMock`、`production_eligible=false`、无 node provenance，
  不伪装为 Firecracker/guest 生成的取消证据；
- judge 退出后 Firecracker API socket 与 vsock 路径均不存在。

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
OPENOJ_RUNTIME_OFFLINE=1 OPENOJ_RUNTIME_CACHE=/tmp/openoj-algorithm-c-cache bash infra/runtime-images/algorithm-c/provision.sh
(cd infra/runtime-images/algorithm-c/out && sha256sum --check --strict manifest.sha256)
OPENOJ_REQUIRE_KVM=1 OPENOJ_TEST_DATABASE_URL=<PostgreSQL 18> cargo test -p openoj-control-plane --test process_e2e real_microvm_cancellation_wins_late_result_and_reclaims --locked -- --nocapture --test-threads=1
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
DATABASE_URL=<PostgreSQL 18> OPENOJ_TEST_DATABASE_URL=<same> OPENOJ_REQUIRE_KVM=1 cargo test --workspace --all-targets --locked
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
# change-openoj-sandbox、develop-openoj、verify-openoj 的 quick validator
```

聚焦取消测试在 16.16 秒通过。完整 workspace 共 152 个测试通过、0 失败；其中 9 个 process
E2E 在 18.86 秒通过，严格 runtime KVM smoke 在 1.86 秒通过，17 个 PostgreSQL
control-spine 测试通过。`cargo deny` 只有既有 duplicate-version 警告。测试后无 Firecracker
进程残留；临时 PostgreSQL 已删除，生成的 runtime outputs 未进入 Git。

精简原始证据位于 `artifacts/2026-08-25-algorithm-c-cancellation-race.txt`，SHA-256 为
`4ae00f9d6c702f4bcd147a6c703adf051672ea77dac12e8c932065886e583ab6`。

## 安全影响、回退与未验证项

本变更只增加测试与验证证据，不修改协议、schema、migration、生产路径、网络、设备、凭证、
argv 或信任边界。回退只需移除该回归测试与验证记录，不影响持久化数据或运行时兼容性。

当前 judge executor 在一次同步 `execute` 前续租一次，执行中不会轮询 control-plane，也没有可被
外部触发的取消句柄。因此取消后 microVM 仍运行到 guest stage 的 8 秒 wall-time deadline，随后
才 teardown 并尝试提交迟到结果。本记录不声称即时停止、guest `Cancel` 消息消费、执行中周期
续租、CLI/HTTP 取消入口、租约丢失即时终止、production jailer/cgroup/seccomp/watchdog，亦不覆盖
CPU/内存/process/disk/I/O/无限输出和并发 guest 的恶意 workload 宿主存活矩阵。这些保持
`Unverified`，运行中即时中断是下一 PR 的实现目标。
