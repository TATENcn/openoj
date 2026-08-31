---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-25
applies_to: validation evidence
references:
  - ../requirements/acceptance.md
  - ../development/testing.md
---

# 验证证据

本目录保存可审查验证摘要；大型原始日志、镜像和二进制保存在受控 Artifact 系统，文档记录位置与内容摘要。

每条记录必须包含：

- 日期、commit 和关联需求/验收/威胁 ID。
- 操作系统、内核、CPU、内存、虚拟化和负载条件。
- Firecracker、kernel、rootfs、guest agent 和 Runtime 版本/摘要。
- debug/release 构建和完整命令。
- 输入 workload、预热状态、重复次数和采样方法。
- 结果、原始数据位置、摘要、异常和未验证项。

设计目标不得写入此目录冒充结果。新证据不删除旧证据；过期记录标记替代关系和适用范围。

## 记录索引

- `2026-08-16-phase-0-governance.md`：Phase 0 文档与 Agent 治理基线。
- `2026-08-16-p0-evaluation-kernel.md`：P0-A schema-first 内存评测内核。
- `2026-08-17-p0b-durable-control-spine.md`：P0-B PostgreSQL 持久化控制脊柱。
- `2026-08-17-p0c-judge-control.md`：P0-C UDS Judge Control 与真实进程闭环；含失败路径回归。
- `2026-08-17-p0d-expired-lease-recovery.md`：P0-D 过期租约恢复，sweeper 接入 control-plane 进程路径并验证端到端强杀恢复。
- `2026-08-18-kvm-firecracker-boot.md`：本机 KVM/Firecracker 启动验证，执行平面目标环境证据起点。
- `2026-08-22-microvm-lifecycle-safety.md`：microVM 失败态回收、Attempt 级 VMM 所有权与不完整 production profile 拒绝的无 KVM 回归证据。
- `2026-08-23-algorithm-c-runtime-smoke.md`：`algorithm-c` 镜像供应、guest 上传摘要、严格 KVM smoke 与当前无 KVM 的未验证边界。
- `2026-08-25-algorithm-c-runtime-kvm.md`：修复 guest 文件系统启动后，在真实 KVM 上验证 C 编译运行、编译失败、运行超时与幂等回收。
- `2026-08-25-algorithm-c-process-e2e.md`：在真实 KVM 上验证 CLI、PostgreSQL、UDS、judge-node、Firecracker、宿主检查和持久化结果的固定 development Artifact 闭环。
- `2026-08-25-algorithm-c-process-failures.md`：真实进程闭环持久化 CompileError 与 TimeLimitExceeded，保留失败证据并跳过后续阶段。
- `2026-08-25-algorithm-c-cancellation-race.md`：真实 microVM workload 期间验证取消终态优先、幂等重放、迟到结果 fencing 与有界最终回收，并明确即时中断仍未实现。
