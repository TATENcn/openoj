---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: repository governance and agent operations
references:
  - ../../../AGENTS.md
  - ../../../CONTRIBUTING.md
  - ../../README.md
---

# ADR-0003：事实源驱动的仓库治理

## 背景与需求

OpenOJ 计划由人工与 Agent 长期协作。仅依赖对话上下文或一份巨大的 Agent 指令会导致规则漂移、重复和无法验证。

## 候选方案

- 仅使用 README/聊天约定：维护简单，但不可执行、不可审计。
- 把全部规则放入 AGENTS：入口集中，但上下文膨胀并重复领域文档。
- 事实源索引 + 精简 AGENTS + 专项 Skills + CI：需要初始治理成本，但边界清晰且可自动验证。

## 决策

采用第三种方案。每个领域只有一个事实源；`AGENTS.md` 负责导航和不可破坏边界；`skills/` 编排可重复工作；CI 验证机械规则；高风险决策保留人工审批。

由于当前运行环境中的 `.agents/` 为只读托管目录，仓库 Skills 暂存于根 `skills/`。未来切换自动发现路径必须通过受控迁移更新所有引用。

## 后果

- 文档变更是功能变更的一部分，而不是事后补充。
- Skill 必须保持精简且不能成为第二事实源。
- Agent 可以自主执行低风险、范围明确的实现步骤，但不能扩大 Git、发布、安全和生产权限。
- 需要维护文档链接、状态、Skill frontmatter 和任务场景验证。

## 迁移与回退

若未来工具支持标准仓库级 Skill 目录，可移动 `skills/` 并同步 `AGENTS.md`、CI 和文档链接；Skill 名称和行为保持稳定。

## 验证

由 `ACC-F0-001`、`ACC-F0-005`、`ACC-F0-006` 和实际 Agent 任务前向测试验证。
