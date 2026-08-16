---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: repository documentation
references:
  - ../AGENTS.md
  - ../CONTRIBUTING.md
---

# OpenOJ 文档治理与事实源

本文是 OpenOJ 正式文档入口。文档定义目标与约束，代码实现约束，测试和验证记录提供证据；三者冲突时不得静默选择一方，必须在同一变更中消除冲突。

## 事实源

| 领域 | 唯一事实源 | 变更时必须检查 |
|---|---|---|
| 项目愿景、范围和术语 | `product/` | 产品边界、非目标、公开描述 |
| 功能与非功能需求 | `requirements/` | 稳定 ID、验收项、测试映射 |
| 总体架构和模块边界 | `architecture/` | 数据流、依赖方向、部署和风险 |
| 不可逆或高迁移成本决策 | `architecture/decisions/` | ADR、替代方案、迁移与回退 |
| 开放评测协议 | `protocol/` | canonical schema、版本、兼容性 |
| 威胁与安全控制 | `security/` | 信任边界、拒绝路径、验证证据 |
| 插件契约 | `plugins/` | 能力、生命周期、隔离和版本 |
| 工程与测试规则 | `development/` | 本地流程、代码、依赖、文档和门禁 |
| Git、Commit、PR、发布 | `../CONTRIBUTING.md` | 分支、提交、审查和发布策略 |
| Agent 顶层行为 | `../AGENTS.md` | Skill 路由、授权和不可破坏边界 |
| 运维与部署 | `operations/` | 支持环境、升级、回收和故障处理 |
| 实测证据 | `validation/` | 环境、命令、数据、限制和有效期 |
| 许可证和人工审批 | `governance/` | 分发、贡献来源和高风险批准 |

`README.md` 只提供项目入口；Skill 只编排流程。两者不得重新定义上述事实。

## 文档状态

规范文档必须在 YAML frontmatter 中声明以下状态之一：

- `Proposed`：可讨论，不构成兼容性承诺，也不得被描述为已实现。
- `Accepted`：当前实现必须遵循；变更需要同步测试，必要时新增 ADR。
- `Implemented`：已有实现与自动化测试，但不表示真实目标环境已验证。
- `Validated`：在声明环境中执行验收并留下可追踪证据。
- `Deprecated`：不再用于新实现，必须指向替代文档或决策。

`Accepted` 不自动升级为 `Implemented`，CI 通过也不自动升级为 `Validated`。

## 规范性语言

- “必须/不得”表示不可违反的约束。
- “应”表示除非有被记录和批准的例外。
- “可以”表示可选行为。
- 避免使用“尽量”“一般”“适当”“最好”等无法验收的词定义约束。

## 稳定 ID

| 前缀 | 含义 | 示例 |
|---|---|---|
| `FR-` | 功能需求 | `FR-JUDGE-001` |
| `NFR-` | 非功能需求 | `NFR-SEC-001` |
| `ACC-` | 验收项 | `ACC-F0-001` |
| `THR-` | 威胁 | `THR-HOST-001` |
| `CTL-` | 安全控制 | `CTL-ISOLATION-001` |
| `RISK-` | 项目或架构风险 | `RISK-PROTOCOL-001` |
| `ADR-` | 架构决策 | `ADR-0001` |

ID 一旦进入 `Accepted` 文档不得重排或复用。废弃内容保留 ID 并标记状态；Commit、PR、测试和验证记录按 ID 引用，不按段落位置引用。

## 修改规则

- 每个事实只在一个文档定义，其他位置使用相对链接。
- 规范与实现必须在同一 PR 同步；设计先行时保持 `Proposed` 或 `Accepted`，不得伪装为实现。
- 新依赖同步技术栈和供应链影响；新风险同步风险清单。
- 改变公共协议、数据格式、安全边界或不可逆承诺时新增 ADR。
- Accepted ADR 不改写结论；改变决策时新增 ADR 并标记替代关系。
- 文档重命名时全仓搜索反向引用；相对链接必须从当前文件解析。
- Mermaid 图只表达一个明确的数据流或边界，异常与降级行为必须由正文定义。
- 真实结果只写入 `validation/`；目标值、预期和实测值必须分开。

## 语言

Phase 0 以中文规范文档作为事实源，代码符号、协议字段、Commit subject 和机器接口使用英文。未来翻译必须声明 canonical 版本和同步状态，不允许中英文规范独立演进。
