# OpenOJ repository instructions

本文件作用于整个仓库。目标是让人工开发者和 Agent 在信息不完整时仍能找到唯一事实源，保持评测正确性、安全边界和可验证交付。

## 开始任务

1. 阅读 `CONTRIBUTING.md` 和 `docs/README.md`。
2. 从 `docs/requirements/` 找到稳定需求 ID 和验收项。
3. 按任务类型阅读对应事实源和 Accepted ADR。
4. 按下表读取并执行最小必要 Skill。
5. 检查 `git status`、当前分支和相关 diff，保留不属于当前任务的修改。
6. 没有需求、验收边界或必要决策时，先补文档或停止并报告。

## Skill 路由

| 任务 | Skill |
|---|---|
| 普通功能、修复和重构 | `skills/develop-openoj/SKILL.md` |
| 独立验证、审计和交付检查 | `skills/verify-openoj/SKILL.md` |
| crate、服务、线程、数据流或信任边界变化 | `skills/change-openoj-architecture/SKILL.md` |
| 公开协议、Schema、SDK 或 guest/host 消息变化 | `skills/evolve-openoj-protocol/SKILL.md` |
| Firecracker、jailer、网络、磁盘或资源隔离变化 | `skills/change-openoj-sandbox/SKILL.md` |

多个 Skill 同时适用时，依次处理架构决策、安全/协议专项、实现和独立验证。Skill 是流程，不是事实源；与正式文档冲突时必须停止并消除冲突。

## 不可破坏的边界

- 不可信代码的编译和运行必须位于受支持的隔离执行边界内；生产路径不得回退到宿主机直接执行。
- 控制平面不得直接启动用户进程，必须通过 execution plane 和 judge node。
- guest 内不得存在平台凭证、数据库凭证、对象存储长期密钥或发布密钥。
- microVM 默认无网络；开放网络必须经过显式策略、配额、审计和 ADR/安全评审。
- 题目包、checker、interactor、generator、插件、归档文件和 AI 参数都属于不可信输入。
- 外部协议以 `docs/protocol/` 指向的 canonical schema 为准；不得手写两套相互漂移的类型。
- 已发布数据库 migration 不得改写，只能新增前向 migration。
- 不可信插件不得以原生动态库加载进核心进程。
- 不得把设计目标写成已验证结论；没有环境和证据时写 `Unverified`。
- 不得在日志和指标标签中记录密钥、原始用户代码、隐藏测试数据或高基数不可信输入。

## Git 授权与协作

- Agent 未经用户明确授权不得创建或切换分支、暂存、提交、push、rebase、merge、打 tag 或改写历史。
- 每次 Git 修改操作前重新检查工作区、分支、暂存区和相关 diff，不假设共享工作区状态未改变。
- 获准暂存时只按路径暂存本任务文件，不得使用 `git add .`、`git add -A` 等宽泛命令。
- 保留并避让用户或其他协作者的修改，不得覆盖、回滚或夹带进入当前变更。
- 禁止破坏性 Git 操作和对共享历史 force-push；具体规则以 `CONTRIBUTING.md` 为准。

## 变更联动

- 改需求：同步验收项和受影响设计。
- 改依赖方向、进程边界、线程模型或信任边界：同步架构文档；不可逆变化新增 ADR。
- 改协议：同步 canonical schema、文档、生成绑定、producer/consumer、版本和兼容测试。
- 改 guest/host 接口：同步两端、畸形输入、取消、超时和版本不匹配测试。
- 改 Firecracker 或资源策略：同步威胁模型、运维约束和恶意 workload 测试。
- 改权限或插件能力：同步能力注册、拒绝路径、配额、审计脱敏和宿主存活测试。
- 改持久化：新增前向 migration，测试空库、旧版本升级、失败和重复运行。
- 改 runtime image：记录工具链、来源、摘要、SBOM 和标准回归结果。

## 代码与验证

- 遵循 `docs/development/coding-standards.md` 和 `docs/development/testing.md`。
- 当前 Phase 0 至少运行 `bash scripts/check-docs.sh` 和所有 Skill 的 `quick_validate.py`。
- Rust workspace 建立后，默认门禁必须包含 fmt、check、Clippy `-D warnings`、测试和依赖策略。
- 不删除、忽略或弱化失败测试来通过门禁。
- 性能结论必须来自 release 构建，并记录硬件、内核、KVM、Firecracker、负载和原始数据。

## 完成定义

变更必须目标单一、可审查、可回退；实现和失败路径完整；相关测试通过；事实源、ADR、安全、协议和运维文档按联动规则更新；交付说明列出实际命令、结果和未验证项。
