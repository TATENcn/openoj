---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: quality attributes
references:
  - README.md
  - acceptance.md
  - ../security/threat-model.md
---

# 非功能需求

## 安全

分组理由与边界：平台的输入、执行器、插件和外部集成均可能恶意，安全目标是限制影响范围而不是声明绝对安全。P0 必须验证生产执行边界；未知硬件侧信道和未支持环境保持残余风险，不得被 CI 通过掩盖。

### NFR-SEC-001：不可信输入模型

状态：Accepted；阶段：全部。

验收：`ACC-P0-002`、`ACC-P0-005`、`ACC-P0-014`、`ACC-P3-001`、`ACC-P4-001`。

系统必须把 Submission、题目包、测试数据处理器、checker、interactor、generator、插件、归档、外部响应和 AI 参数视为不可信输入，除非它们经过明确供应链信任流程。

### NFR-SEC-002：纵深隔离

状态：Accepted；阶段：P0。

验收：`ACC-P0-002`、`ACC-P0-005`、`ACC-P0-015`。

生产执行必须至少组合 KVM/Firecracker 边界、jailer 或等价约束、最小权限、namespace、cgroup、seccomp、资源上限和宿主 watchdog。任何单层失效不得直接赋予控制平面或其他租户权限。

### NFR-SEC-003：默认拒绝

状态：Accepted；阶段：全部。

验收：`ACC-P0-005`、`ACC-P0-019`、`ACC-P3-001`、`ACC-P4-001`。

网络、宿主文件、凭证、外部服务、插件能力和管理操作必须默认拒绝，并通过显式策略授予最小范围、期限和配额。

### NFR-SEC-004：秘密与数据最小化

状态：Accepted；阶段：全部。

验收：`ACC-P0-005`、`ACC-P0-006`。

guest 不得持有平台长期密钥；日志、错误、指标和审计不得泄漏凭证、隐藏数据、宿主路径或原始敏感内容。

## 正确性与可靠性

分组理由与边界：共享宿主、重试和外部故障会影响结果一致性，因此必须固定语义输入、使用幂等状态转换并限制所有资源。可复现不等于不同硬件上的时间指标完全相同，资源偏差需单独量化。

### NFR-REPRO-001：评测可复现

状态：Accepted；阶段：P0。

验收：`ACC-P0-004`。

系统必须固定影响结果的输入、环境、时区、Locale、随机种子和版本，并保留足够来源信息解释历史结果。无法完全复现的资源计量差异必须显式记录。

### NFR-REL-001：幂等与故障恢复

状态：Accepted；阶段：P0。

验收：`ACC-P0-003`、`ACC-P0-018`。

对外写 API、调度、Attempt 创建、结果提交和 Artifact 注册必须定义幂等键与重试语义。worker 崩溃不得造成任务永久丢失或重复计分。

### NFR-REL-002：有界资源

状态：Accepted；阶段：P0。

验收：`ACC-P0-002`、`ACC-P0-016`。

所有队列、消息、日志、输出、Artifact、定时器、并发和重试必须有界，并定义超限错误、清理与审计行为。

## 性能

分组理由与边界：高性能必须由固定 workload 和真实环境数据定义。Phase 0 不接受猜测阈值；P0 原型先建立基线，再由人工接受具体 SLO。

### NFR-PERF-001：可测量性能

状态：Accepted；阶段：P0。

验收：`ACC-P0-007`。

项目必须定义并自动采集 cold/warm 启动、排队、build/run、结果持久化、单节点吞吐和资源回收的 P50/P95/P99。性能结论必须来自 release 构建和记录完整的宿主环境。

### NFR-PERF-002：初始阈值

状态：Proposed；阶段：P0。

验收：接受具体阈值时分配新的具体 P0 验收 ID，当前不得以该 Proposed 需求通过性能验收。

P0 原型完成后应基于同一 benchmark workload 接受具体 cold/warm 启动、吞吐、计量偏差和回收阈值。在获得基线前不得虚构数值承诺。

## 开放性与维护

分组理由与边界：开放性依靠稳定语义、可追踪供应链和可操作仓库，而不是提前承诺所有集成。P0 建立 alpha 协议和维护基线，稳定协议与插件兼容属于 P3。

### NFR-OPEN-001：传输无关协议

状态：Accepted；阶段：P0 基础、P3 稳定。

验收：`ACC-P0-001`、`ACC-P3-002`。

公开评测语义不得绑定单一语言、数据库或内部队列。外部表示优先采用可读 Schema，内部传输可以优化但必须保持显式转换。

### NFR-MAINT-001：Agent 可操作仓库

状态：Accepted；阶段：Phase 0。

验收：`ACC-F0-001`、`ACC-F0-002`、`ACC-F0-004`、`ACC-F0-005`、`ACC-F0-006`。

仓库必须通过事实源索引、稳定 ID、AGENTS 指令、Skills、确定命令和完成定义，使新 Agent 在不依赖历史对话的情况下完成受限任务。

### NFR-OBS-001：端到端可观测

状态：Accepted；阶段：P0。

验收：`ACC-P0-006`。

API 请求、Evaluation、Attempt、Stage、judge node 和 microVM 必须使用稳定关联 ID。指标标签必须低基数，不得直接使用用户输入。

### NFR-SUPPLY-001：供应链可追踪

状态：Accepted；阶段：P0。

验收：`ACC-P0-008`。

源码依赖、构建工具、kernel、rootfs、runtime image 和发布产物必须记录版本、来源、摘要和许可证状态；正式发布需要 SBOM 与签名策略。

### NFR-COMPAT-001：受控演进

状态：Accepted；阶段：全部。

验收：`ACC-P0-003`、`ACC-P3-003`。

公共协议、插件契约、持久化格式和滚动升级组合必须有明确版本、兼容范围、弃用流程和不支持时的显式错误。
