---
status: Proposed
owners: OpenOJ protocol maintainers
last_reviewed: 2026-08-16
applies_to: open evaluation protocol
references:
  - versioning.md
  - v0alpha1.md
  - ../product/glossary.md
  - ../requirements/functional.md
---

# Open Evaluation Protocol

OpenOJ 计划定义传输无关的开放评测协议。协议描述评测语义，不暴露 PostgreSQL 表、内部队列或 Firecracker API。

## 核心对象

- `EvaluationRequest`：Problem Version、Submission、Runtime、Plan、Policy 和 capability 要求。
- `Artifact`：内容摘要、媒体类型、大小、敏感性和受控位置。
- `Stage`：有类型、可依赖、可取消的评测步骤。
- `Evidence`：结构化事实或 Artifact 引用。
- `EvaluationResult`：状态、Verdict/Score、Attempt、阶段、指标、诊断和来源。
- `Capability`：实现支持并被授权的能力。
- `Event`：排队、开始、阶段完成、重试、取消和终态通知。

外部 canonical 表示采用 JSON Schema；内部高频通信可以采用 Protobuf/gRPC，但必须通过显式转换保持同一语义。首个请求/结果 schema 已在 `schemas/openoj/v0alpha1/open-evaluation.schema.json` 实现，规范语义见 `v0alpha1.md`。当前协议仍是 alpha，不构成稳定兼容承诺。

## 设计原则

- 所有消息有版本、大小上限和未知值行为。
- Artifact 与大内容分离，消息只包含元数据和引用。
- 状态机和幂等语义属于协议，不依赖某个数据库实现。
- 错误可分类、可脱敏、可扩展，不把日志全文当协议。
- 算法题是第一个 Profile，不是核心对象的特殊硬编码。
- 在至少两个 Profile 和 conformance suite 验证前保持 alpha。
