---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: requirements governance
references:
  - functional.md
  - non-functional.md
  - acceptance.md
  - ../product/scope.md
---

# 需求基线

OpenOJ 使用稳定 ID 管理需求与验收。需求描述产品必须具备的能力，架构文档描述如何满足，测试和 `docs/validation/` 记录证据。

## 需求结构

每项 Accepted 需求必须直接或通过所在的命名分组包含：

- 稳定 ID 和状态。
- 规范性描述。
- 适用阶段。
- 设计理由或风险。
- 明确非目标或边界；分组边界不得覆盖条目自身更窄的阶段和语义。
- 可执行验收映射。

没有验收方法的目标只能保持 `Proposed`。一个实现可以满足多个需求，但不得用单个模糊测试代替每项关键安全和失败行为。

## 变更规则

- 新行为先增加或修改需求，再实现代码。
- ID 不按优先级重排；删除时标记 Deprecated。
- 破坏兼容、安全或数据语义的需求变化需要 ADR。
- Commit 和 PR 使用 `Refs:` 引用需求或验收 ID。
- 未来目标不能被当前代码存在与否自动改成 Implemented。
