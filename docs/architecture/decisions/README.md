---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: architecture decision records
references:
  - ../../governance/change-control.md
---

# Architecture Decision Records

ADR 记录长期约束兼容性、安全、持久化、部署、许可或迁移成本的决策。普通实现细节和可轻易回退的局部选择不需要 ADR。

文件名使用 `NNNN-short-title.md`，编号递增。Accepted ADR 不改写结论；改变决策时新增 ADR，并在旧文件中标记 `Superseded by ADR-NNNN`。

模板：

```markdown
---
status: Proposed | Accepted | Deprecated
owners: OpenOJ maintainers
last_reviewed: YYYY-MM-DD
applies_to: <scope>
references: []
---

# ADR-NNNN: 标题

## 背景与需求

## 候选方案

## 决策

## 后果

## 迁移与回退

## 验证
```

ADR 必须列出真实候选，并说明安全、性能、兼容、运维、许可证和维护影响。只有具备权限的人类维护者可以接受或替代 ADR。
