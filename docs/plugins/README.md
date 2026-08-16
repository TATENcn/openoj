---
status: Proposed
owners: OpenOJ plugin maintainers
last_reviewed: 2026-08-16
applies_to: plugin model
references:
  - ../requirements/functional.md
  - ../security/threat-model.md
  - ../protocol/README.md
---

# 插件模型基线

OpenOJ 的可扩展性必须建立在稳定契约和能力限制上，不允许插件直接获得核心进程权限。

## 扩展等级

1. Wasm Component/WASI 插件：轻量转换、检查、评分和事件处理。
2. 隔离服务插件：认证、通知、存储和外部平台集成。
3. microVM evaluator：完整工具链、工程项目或不可信高级评测。

## 必备契约

插件必须声明身份、版本、契约版本、输入输出 Schema、capability、scope、资源预算、超时、幂等、失败策略和签名/来源。宿主默认拒绝未声明或未授权能力。

钩子分为观察型事件、受限同步拦截和 Evaluation Stage。必须定义顺序、重试、超时、失败是否阻断、事务边界、递归限制和审计；插件不能通过 hook 绕过领域状态机。

WASI/Component Model 仍需由 OpenOJ 固定受支持版本。插件 SDK 和 manifest 在 P1 设计，经过安全测试和第二个 Profile 后再稳定。
