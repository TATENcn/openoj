---
status: Implemented
owners: OpenOJ maintainers
last_reviewed: 2026-08-22
applies_to: Firecracker failure reclamation, judge-node Attempt ownership, and production-profile rejection
references:
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../requirements/non-functional.md
  - ../security/threat-model.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - 2026-08-20-p0d-firecracker-boot.md
---

# microVM 生命周期安全验证（2026-08-22）

## 范围与目标

本记录验证 `0c45e8ad8a23437354bbde95d0386ba64811abed` 及其前两个提交对
`FR-JUDGE-003`、`ACC-P0-015/016/017` 和 `CTL-RECLAIM-001` 的局部落实：

- control API 失败后的 `Failed` VM 仍能进入幂等回收；
- judge-node 在单个 Attempt 内同时持有 guest session 与 VMM lease，Attempt 返回时回收，
  常驻 worker 可继续处理下一 Attempt；
- 不完整的 production isolation profile 即使配置 jailer 也 fail closed，结果不标记为
  production eligible。

本记录不验证完整 `ACC-P0-015`，也不替代真实 KVM、jailer 或恶意 workload 证据。

## 环境

- Arch Linux x86_64，kernel `7.1.8-zen1-3-zen`；
- AMD Ryzen 9 9950X3D，8 个可见 CPU，AMD-V；15 GiB 内存；
- Rust/Cargo `1.97.1`，Firecracker `1.16.1`；
- `/dev/kvm` 不存在；`DATABASE_URL` 与 `OPENOJ_TEST_DATABASE_URL` 未设置；
- debug 构建；未运行性能 workload。

## RED → GREEN 回归

- `failed_phase_remains_reclaimable_until_terminated` 首先在
  `needs_teardown()` 断言失败，修复后通过；
- `terminate_reclaims_child_after_control_failure` 首先证明 `terminate()` 从 `Failed`
  返回错误，修复后真实启动的受控 `sleep` 子进程被 kill、wait 并进入 `Terminated`；
- `each_attempt_reclaims_vm_and_keeps_executor_reusable` 首先证明成功 Attempt 返回后
  VMM lease 未回收，修复后连续两个 Attempt 各回收一次；
- `bootstrap_failure_inside_worker_runtime_returns_error_without_panicking` 首先复现嵌套
  Tokio runtime panic，修复后在多线程 worker runtime 内返回分类错误；
- `production_with_jailer_remains_rejected_until_isolation_profile_is_complete` 首先证明仅有
  jailer 路径会错误接受 production，修复后 fail closed。

## 门禁结果

- `cargo fmt --all -- --check`：通过；
- `cargo check --workspace --all-targets --locked`：通过；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过；
- `cargo test --workspace --all-targets --locked --exclude openoj-storage --exclude openoj-cli --exclude openoj-control-plane`：
  109 个测试函数通过，其中真实 KVM boot 测试因 `/dev/kvm` 缺失在测试体内明确自跳过；
- `cargo deny --locked check advisories bans sources`：通过，存在既有 duplicate dependency warning；
- `bash scripts/check-docs.sh`：通过；
- `python3 scripts/check-workspace.py`：通过；
- 使用临时 PyYAML 6.0.3 环境对全部 5 个 `skills/*` 运行 `quick_validate.py`：通过。

完整 `cargo test --workspace --all-targets --locked` 在
`migrate_submit_replay_and_status_use_the_durable_store` 处因缺少 `DATABASE_URL` 停止；这是环境阻塞，
不得解释为数据库套件通过或本次代码失败。

## 未验证与残余风险

- PostgreSQL、control-plane process E2E 与强杀恢复测试未运行；需要隔离 PostgreSQL 18；
- 真实 KVM bootstrap、guest↔host vsock 往返、guest agent runtime image 和失败回收未运行；
- 独立 uid/gid、cgroup、namespace、seccomp、宿主 watchdog 尚未实现，production profile
  因此保持拒绝；
- CPU/内存/进程/磁盘/I/O/无限输出、取消竞态、VMM deadlock、宿主和并发任务存活未验证；
- 无 release 构建性能结论，无生产安全结论。
