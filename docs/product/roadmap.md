---
status: Proposed
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: delivery sequence
references:
  - scope.md
  - ../requirements/acceptance.md
---

# 路线图

## Phase 0：治理与设计基线

建立事实源、需求 ID、ADR、安全模型、开发流程、Skills 和文档门禁。完成标准见 `ACC-F0-*`。

## P0：单机算法题垂直切片

完成提交、调度、Firecracker、guest 编译执行、标准检查和结果返回。只支持少量语言和单节点生产 Profile。

## P1：安全和性能强化

完成镜像供应链、快照/预热策略、恶意 workload、模糊测试、资源公平性、基准回归和故障恢复。

## P2：分布式执行

加入多节点调度、配额、节点排空、可靠重试、对象存储、滚动升级和版本兼容矩阵。

## P3：开放协议与插件生态

在至少两个评测 Profile 验证后稳定公开协议，发布 SDK、conformance suite 和能力受限插件宿主。

## P4：AI 与通用评测实验

分别选择一个多模态试卷和一个工程项目评测场景验证通用工作流；AI 输出默认作为可追踪证据或建议，不绕过平台策略。

路线图不是时间承诺。每一阶段以验收和证据推进，不以文档中的未来勾选框声明完成。
