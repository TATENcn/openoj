---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: service topology and trust boundaries
references:
  - ../overview.md
  - ../../security/trust-boundaries.md
---

# ADR-0002：模块化控制平面与独立执行平面

## 背景与需求

OpenOJ 既需要快速形成端到端闭环，也需要把互联网 API、数据库和不可信执行分隔。过早拆分微服务会增加一致性和运维成本，把所有能力放进单进程又会破坏安全边界。

## 候选方案

- 单进程单体：开发快，但控制平面直接接触 VMM 和不可信执行。
- 全微服务：边界明显，但 P0 的部署、协议和故障复杂度过高。
- 模块化控制平面 + 独立 judge node：保留进程/主机安全边界，在控制侧维持简单事务模型。

## 决策

P0 使用模块化 Rust 控制平面、PostgreSQL 可靠任务/outbox 和独立 judge node。控制平面内部按领域 crate 隔离，但不为每个模块创建网络服务。judge node 通过版本化任务契约接入，不直接依赖控制平面数据库内部 Schema。

## 后果

- 控制平面可以作为单个部署单元演进。
- judge node 可以独立扩缩、排空和升级。
- 任务和结果必须幂等，并容忍重复投递。
- 将来拆分服务需要基于故障域、扩展或所有权证据，而不是架构审美。

## 迁移与回退

内部模块通过应用接口隔离。若未来拆分，先稳定用例边界和数据所有权，再把调用替换为版本化消息/RPC。

## 验证

由 `ACC-P0-001`、`ACC-P0-003` 和端到端 tracing 验证。
