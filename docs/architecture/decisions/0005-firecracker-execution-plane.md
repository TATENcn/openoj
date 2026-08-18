---
status: Proposed
owners: OpenOJ maintainers
last_reviewed: 2026-08-18
applies_to: execution plane architecture for the P0 vertical slice
references:
  - 0001-rust-and-firecracker.md
  - 0002-modular-control-and-execution-planes.md
  - ../../product/scope.md
  - ../../security/trust-boundaries.md
  - ../../security/threat-model.md
  - ../../requirements/acceptance.md
  - ../../architecture/overview.md
  - ../../superpowers/specs/2026-08-17-p0d-execution-plane-design.md
---

# ADR-0005：Firecracker 执行平面架构

## 背景与需求

`ADR-0001` 已固定 Rust + Firecracker microVM 作为生产不可信代码的虚拟化边界；P0-C/P0-D 完成 judge node 与 control-plane 的任务领取、租约、恢复与结果提交。但 judge node 尚不能执行不可信代码：没有 Firecracker/jailer 适配器、评测器、guest agent 或 runtime image，`ACC-P0-001` 端到端闭环缺失执行段。

本决策定义执行平面的组件边界、jailer 配置、guest/host 传输、资源隔离、运行时供应链与最小切片，落实 `ACC-P0-001/002/005/007/008/010/015/016/017`。

## 候选方案

- **Firecracker + jailer**（延续 ADR-0001）：轻量 microVM、固定设备面、与 RPC 相同的最小化哲学；需要 Linux/KVM、jailer 配置与供应链治理。
- **QEMU/KVM**：功能全但攻击面与启动/密度成本更高，偏离已确定方向。
- **runc/容器**：ADR-0001 已因多租户隔离假设不足而拒绝。
- **进程级 seccomp/namespace**：不足以隔离不可信编译与运行所需的文件、设备与进程面。

虚拟化选择已由 ADR-0001 固定，本决策不重开。开放决策是：judge node 如何驱动 jailer/VMM、guest 如何通信、资源如何量化、runtime image 如何溯源、以及最小可交付切片。

## 决策

### 组件边界（新 crate）

- `openoj-firecracker`：唯一持有平台/特权代码的系统适配器。封装 jailer/VMM 启动、chroot/uid/gid、cgroup、seccomp、vsock、block 设备、生命周期状态机与幂等回收。不得包含用户、竞赛或计分逻辑。
- `openoj-evaluator`：把 Evaluation Plan 展开为 `prepare -> build -> run -> check -> aggregate -> finalize` 阶段，逐阶段记录状态、资源指标、有界诊断与 Evidence/Artifact 引用；阶段失败不得伪造后续成功。
- `openoj-guest-agent`：guest 内最小 agent，只实现有限、版本化 vsock 命令协议（上传输入、编译、运行、回传阶段证据、响应取消），不提供通用 shell。

### guest/host 传输

- 使用 Firecracker vsock，消息有版本、长度上限、状态机与超时约束（`ACC-P0-009`、`ACC-P0-016`）。
- guest 输出一律重新按不可信输入处理；最终 Verdict/Score 由宿主侧 evaluator 根据证据形成，guest 自报成功不构成终态（`ACC-P0-010`、信任边界 guest→结果）。

### 隔离与资源（`ACC-P0-002/015/016`）

- jailer：独立 uid/gid、chroot、`0700`/`0600` 目录、cgroup（cpu/memory/pids/io）、namespace、seccomp 过滤器。
- Firecracker machine-config：vcpu/内存上限；evaluator watchdog 强制墙钟/阶段超时与强制回收。
- guest 默认无 TAP 网络设备（`ACC-P0-005`）；开放网络必须走显式策略/配额/审计/ADR/安全评审。
- 生产配置必须证明同时启用上述强制层；缺失任一强制层时 fail closed，不得静默切换 mock/容器 executor（`ACC-P0-015`）。

### 运行时供应链（`ACC-P0-008`）

- kernel、rootfs、guest agent 与 toolchain 为不可变、内容摘要寻址对象；记录构建来源、依赖锁定、许可证状态与 SBOM。
- 摘要或架构不匹配时执行被显式拒绝；缓存按摘要与敏感级别隔离，不允许跨任务数据泄漏。

### 最小切片（`ACC-P0-001`）

- 固定一个 Runtime（单语言）、固定 kernel/rootfs/guest agent，从 CLI 经 control-plane 调度到 judge node，在 Firecracker guest 内 build/run，宿主侧 check/aggregate，返回结构化结果与来源信息。
- development mock 保持显式且仅测试可见，生产配置不得接受 mock（延续 ADR-0001）。

## 后果

- judge node 获得宿主级能力（uid/gid、cgroup、seccomp、vsock、block 文件）：信任边界扩大，需威胁模型与安全评审同步。
- kernel/rootfs/guest agent/runtime image 成为一等供应链与兼容性对象。
- 隔离与性能结论只能在真实 KVM 上以 release 构建、固定 workload 与原始数据取得；本机已具备 KVM + Firecracker 1.16.1，可在原型期提供目标环境证据。
- 新协议（guest/host vsock）需按 `evolve-openoj-protocol` 流程维护 canonical schema 与版本矩阵。

## 迁移与回退

- 执行器通过内部 trait 与版本化任务协议隔离；firecracker adapter 可在不触碰控制平面/协议的前提下独立演进或替换。
- 新增 runtime image 版本为前向增加；旧 Evaluation 永远解析到不可变版本。
- 回退触发：多重有效租约、能力错投、guest 逃逸、取消后成功计分、供应链摘要不匹配、资源无限增长。停止新 VMM 写入，保留数据库与 Attempt 历史；回滚到最后一个兼容构建。

## 验证

实现阶段以 `ACC-P0-001/002/005/007/008/010/015/016/017` 为目标，在真实 KVM 上执行恶意 workload、资源耗尽、取消竞态、重复回收与宿主存活测试，并以 release 构建记录环境与原始数据。本机 KVM（API v12，8 vCPU）与 Firecracker 1.16.1 已通过 hello 镜像启动验证，可作为目标环境证据起点。
