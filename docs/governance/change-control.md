---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: decisions, RFCs, and human approvals
references:
  - ../architecture/decisions/README.md
  - ../../CONTRIBUTING.md
---

# 变更控制与人工审批

## Issue、RFC 与 ADR

- Issue 用于目标明确、边界局部的功能、缺陷和治理任务。
- RFC 用于跨模块、需要讨论但仍可回退的设计；可以先使用带 `RFC` 标签的 Issue，避免创建空文档体系。
- ADR 用于会长期约束兼容性、安全、数据、部署、许可或迁移成本的决策。

普通实现细节不得滥用 ADR；改变 Accepted ADR 时必须创建替代 ADR。

## 任务契约

跨模块任务应包含：目标、背景、关联 ID、范围、非范围、受影响组件、安全/协议/数据影响、验收、验证、未决问题、Agent 自主范围和人工审批点。

## 必须人工批准

以下事项不能由 Agent 单独决定或仅凭 CI 通过接受：

- 接受、拒绝或替代 ADR。
- 降低安全限制、开放网络或增加宿主权限。
- 公共协议破坏性变更或缩短弃用窗口。
- 可能丢失、重写或无法回滚的数据 migration。
- 修改许可证、DCO/CLA、商标或内容权利策略。
- 修改发布签名、密钥和供应链信任根。
- 忽略、删除或降低安全/兼容性门禁。
- 创建正式版本、发布产物或操作生产环境。
- 将 `Implemented` 提升为 `Validated`，除非存在真实目标环境证据。

Agent 可以准备候选方案、实现、测试和证据，但必须在交付中明确等待何种批准。
