---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: core implementation and untrusted execution
references:
  - ../../product/vision.md
  - ../technology-stack.md
  - ../../security/trust-boundaries.md
---

# ADR-0001：Rust 核心与 Firecracker 执行边界

## 背景与需求

OpenOJ 需要自研高性能控制链路并执行恶意代码。项目发起人明确选择 Rust 和 Firecracker 作为核心方向，需求关联 `FR-JUDGE-001`、`NFR-SEC-002`、`NFR-REPRO-001`。

## 候选方案

- Rust + Firecracker：内存安全核心、明确所有权和轻量 microVM，但需要 Linux/KVM 和复杂宿主治理。
- Rust + 容器隔离：启动简单，但生产多租户攻击面和隔离假设不符合当前目标。
- 其他语言 + 传统 VM：可行但偏离已确定技术方向，冷启动和资源密度成本更高。

## 决策

OpenOJ 核心服务、judge node 和 guest agent使用 Rust。生产不可信构建和运行以 Firecracker microVM 作为主要虚拟化边界，并强制结合 jailer 或等价约束、cgroup、namespace、seccomp、最小权限和宿主 watchdog。

Firecracker 不是完整沙箱声明。开发环境可以提供 mock executor，但必须显式标记，且不得被生产配置接受。

## 后果

- 生产 judge node 需要受支持 Linux、KVM、内核和硬件虚拟化。
- 控制平面可在无 KVM 环境开发，但无法给出真实隔离和性能结论。
- kernel、rootfs、guest agent 与 runtime image 成为供应链和兼容性对象。
- 需要专项恶意 workload、资源耗尽、回收和宿主存活测试。

## 迁移与回退

执行器通过内部 trait 和版本化任务协议隔离。未来可以新增其他 executor，但生产安全等级、能力和结果来源必须显式区分，不能静默回退。

## 验证

实现阶段通过 `ACC-P0-001`、`ACC-P0-002`、`ACC-P0-005` 和后续 KVM benchmark 验证。
