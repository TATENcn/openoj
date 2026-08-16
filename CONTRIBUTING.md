# OpenOJ 贡献指南

本文件是 OpenOJ 分支、Commit、PR、审查与发布协作规则的唯一事实源。人工开发者和 Agent 均必须遵循。

## 分支模型

- `main` 是受保护的主分支，必须始终保持可构建、基础门禁通过并具备明确发布状态。
- 禁止直接在 `main` 开发或 push。普通工作从最新 `main` 创建短生命周期任务分支，通过 PR 合回。
- 人工分支使用 `feat/`、`fix/`、`refactor/`、`test/`、`docs/`、`ci/`、`build/`、`perf/`、`security/` 或 `chore/`。
- Agent 分支使用工具无关的 `agent/<topic>`。如运行环境有更高优先级前缀要求，应遵循环境要求并在 PR 中说明。
- 一个分支只承载一个需求、修复或治理目标。依赖升级、无关格式化和顺手重构必须拆分。
- 出现发布稳定期后才通过 ADR 引入 `release/x.y`；当前不维护永久 `dev` 分支。

### 首次基线例外

仓库完全没有 commit、因此不存在可作为任务分支基线的 `main` 时，允许在用户明确授权后创建且仅创建一个 root baseline commit。该 commit 必须只包含经过完整 Phase 0 门禁的仓库治理基线，并在提交前检查全部待暂存文件。首次 commit 建立后，本例外立即失效，后续所有工作必须遵循任务分支和 PR 流程。

## Git 操作安全

任何创建/切换分支、暂存、提交、rebase、删除、合并、tag 或发布操作前必须检查：

```bash
git status --short --branch
git branch --show-current
git diff
git diff --cached
```

- 新建分支显式指定基线，不依赖未知的当前 HEAD。
- 发现其他协作者修改、意外分支或预期外提交时，先保留并避让。
- Agent 未经用户明确授权不得执行任何改变 Git 状态或远端状态的操作。
- 获准暂存时只能按路径添加本任务文件，禁止宽泛暂存。
- 禁止改写共享分支历史和直接 force-push。个人未共享分支的历史调整也需要明确确认。
- 所有合并通过 PR；默认 squash merge，合并后删除任务分支。

## 什么时候可以提交

一个 commit 不必完成整个产品需求，但必须形成独立、可审查、可回退的完整步骤，并同时满足：

1. 只有一个明确目的。
2. workspace 可构建，不依赖后续 commit 修复半迁移状态。
3. 受影响范围的格式化、lint 和测试通过。
4. 错误、取消、超时和降级路径与主路径一起完成。
5. `AGENTS.md` 规定的文档、协议、安全和迁移联动已完成。
6. 不包含临时调试代码、无追踪 TODO、秘密或无关修改。
7. Commit body 能真实记录动机、验证与未验证项。

出现编译/测试失败、WIP、协议只改一端、迁移不完整、缺少回归测试或必须依赖下一 commit 才可运行时，不得提交到共享历史。

## Commit message

所有 commit 使用以下结构，body 不得省略：

```text
<type>(<scope>): <summary>

<说明动机、边界和取舍，不能只重复 subject。>

Refs: <requirement/acceptance/issue/ADR/security ID>
Tests: <实际执行的命令和结果>
Unverified: <未验证项或 none>
```

允许的 `type`：`feat`、`fix`、`refactor`、`test`、`docs`、`ci`、`build`、`chore`、`perf`、`revert`。

稳定 `scope` 优先使用：`protocol`、`control-plane`、`scheduler`、`judge-node`、`microvm`、`guest`、`runtime`、`evaluator`、`plugin`、`storage`、`api`、`web`、`security`、`observability`、`infra`、`docs`、`repo`。

安全、兼容或迁移变更按需增加：

```text
Security: <信任边界和控制影响>
Compatibility: <API/协议/数据兼容影响>
Migration: <升级、回滚和数据影响>
```

- `Refs:`、`Tests:`、`Unverified:` 必须存在；不适用时说明原因，不得虚构。
- Subject 使用英文祈使句；body 可以使用中文或英文，但同一 commit 内保持一致。
- 不添加自动生成的 `Co-authored-by:` trailer。外部贡献者签署策略由许可证/DCO ADR 决定。

## PR 要求

PR 必须：

- 目标单一并关联稳定需求、验收、风险或 ADR ID。
- 描述范围与非范围、设计取舍、风险和回滚方式。
- 列出实际执行的命令、环境、结果和未验证项。
- 说明安全、协议、数据、插件、运维和许可证影响。
- 对性能结论附 release 构建、环境、方法、基线和原始数据位置。
- 对安全边界变更覆盖拒绝路径、资源耗尽和宿主/其他任务存活。

普通 PR 至少需要一名维护者审查；安全边界、破坏性协议、数据丢失风险、许可证和发布变更需要相应 CODEOWNER 与人工批准。

## 发布

- 正式版本从通过发布门禁的 `main` commit 创建签名 `vX.Y.Z` tag。
- 发布必须包含版本说明、兼容性说明、SBOM、产物摘要、来源和未验证平台。
- 在许可证、签名和供应链策略 Accepted 前，不创建或分发正式产物。
- Agent 不得自行接受 ADR、降低安全门禁、执行破坏性 migration 或创建正式发布。
