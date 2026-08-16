---
status: Proposed
owners: OpenOJ protocol maintainers
last_reviewed: 2026-08-16
applies_to: protocol compatibility
references:
  - README.md
  - ../requirements/non-functional.md
  - ../architecture/decisions/README.md
---

# 协议版本与兼容性

## 版本层级

- 产品版本、HTTP API、开放评测协议、guest/host 协议、插件契约和持久化 schema 分别版本化。
- P0 开放评测协议标记为 `v0alpha`；alpha 允许破坏性变化，但每次变化仍需迁移说明和契约测试。
- 达到稳定版本后使用 SemVer 语义，并定义弃用窗口和滚动升级矩阵。

## 兼容规则

- 新增可选字段通常向后兼容，但必须定义默认和缺失语义。
- 新增 enum 值要求消费者保留 unknown 分支，不能反序列化崩溃或误映射为成功。
- 删除、重命名、改变单位、默认、必填性、状态机或安全含义属于破坏性变化。
- 生产者不得依赖消费者忽略未知安全关键字段。
- 不支持的版本或 capability 必须显式 fail closed，并返回可分类错误。
- canonical schema、文档、生成绑定、fixture 和 conformance tests 在同一变更更新。

## 稳定前置条件

协议只有在算法题和至少一个非算法 Profile 完成实现、独立兼容实现通过 conformance、升级/降级行为明确并获得 Accepted ADR 后才可以声明 1.0。
