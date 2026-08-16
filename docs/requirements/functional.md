---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: product functionality
references:
  - README.md
  - ../product/scope.md
  - acceptance.md
---

# 功能需求

## 领域与版本

分组理由与边界：评测可复现和可靠重试要求逻辑身份、不可变版本和物理执行分离。P0 只定义算法题所需的最小对象，不在这些对象中嵌入竞赛、计费、组织或具体存储实现。

### FR-PROBLEM-001：不可变题目版本

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-004`、`ACC-P0-012`。

系统必须将题目身份与不可变 Problem Version 分离。任何影响题面、测试数据、限制、评测计划或评分语义的变更必须创建新版本并保留内容摘要。

### FR-SUBMISSION-001：提交与执行分离

状态：Accepted；阶段：P0。

验收：`ACC-P0-003`、`ACC-P0-006`、`ACC-P0-012`。

系统必须将 Submission、逻辑 Evaluation 和物理 Attempt 分离。重试不得静默创建新 Submission，Rejudge 必须保留原结果与新结果的来源关系。

### FR-ARTIFACT-001：内容寻址产物

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-005`、`ACC-P0-009`。

评测输入和输出必须通过稳定元数据与内容摘要引用。数据库不得把无限制源码、日志或二进制直接嵌入调度消息。

## 评测与执行

分组理由与边界：不可信工作负载需要阶段化、可取消和可追踪的执行语义。P0 限于批处理算法题和少量固定 Runtime，不承诺交互题、任意工程工作流或多模态产品能力。

### FR-EVAL-001：版本化评测请求

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-006`、`ACC-P0-013`。

系统必须以有版本的结构化请求描述 Submission、Problem Version、Runtime、Evaluation Plan、Policy 和允许能力，并产生有版本的结构化结果。

### FR-JUDGE-001：隔离构建与运行

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-002`、`ACC-P0-005`。

不可信源码的编译和运行必须在 execution plane 的隔离环境中完成；生产路径不得在控制平面或 judge host 直接执行用户命令。

### FR-JUDGE-002：阶段化算法评测

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-010`。

算法题 Profile 必须至少表达 prepare、build、run、check 和 aggregate 阶段，并为每阶段记录状态、资源指标、受限诊断和证据引用。

### FR-JUDGE-003：确定取消与回收

状态：Accepted；阶段：P0。

验收：`ACC-P0-002`、`ACC-P0-003`、`ACC-P0-017`、`ACC-P0-018`。

系统必须定义排队中、运行中和结果持久化期间的取消、超时、节点失联与强制回收行为，并保证重复请求幂等。

### FR-RUNTIME-001：不可变运行时

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-004`、`ACC-P0-008`。

Runtime 必须固定编译器/解释器、依赖、guest agent、kernel、rootfs 以及适用架构的版本或内容摘要。别名可以移动，但历史 Evaluation 必须解析到不可变版本。

## 调度与结果

分组理由与边界：节点和网络会失败，任务会重复投递，因此调度和结果必须依靠租约、幂等与证据而非 exactly-once 假设。P0 不承诺跨区域调度或复杂工作流平台。

### FR-SCHED-001：可靠任务交付

状态：Accepted；阶段：P0。

验收：`ACC-P0-003`、`ACC-P0-018`。

调度必须容忍重复投递、worker 崩溃、租约过期和短暂存储故障。系统不得依赖 exactly-once 假设保证正确性。

### FR-RESULT-001：证据化结果

状态：Accepted；阶段：P0。

验收：`ACC-P0-001`、`ACC-P0-003`、`ACC-P0-006`、`ACC-P0-013`。

Evaluation Result 必须包含协议版本、状态、Verdict/Score、Attempt、环境来源、阶段摘要、资源指标、诊断和 Artifact/Evidence 引用。

### FR-AUDIT-001：不可抵赖审计链路

状态：Accepted；阶段：P0 基础、P4 AI 工具。

验收：`ACC-P0-005`、`ACC-P0-006`、`ACC-P0-011`、`ACC-P4-001`。

安全、权限、调度、Rejudge 和人工覆盖必须在 P0 产生结构化审计事件；AI 工具在 P4 引入后遵循同一审计模型。审计信息必须可关联主体、动作、目标、决策、原因和请求 ID，并执行脱敏。

## 扩展与 AI

分组理由与边界：扩展能力需要在不修改核心领域模型的前提下受控演进。插件稳定化属于 P3，AI 受控评测属于 P4；两者都不能绕过确定性证据、安全策略和人工审批。

### FR-PLUGIN-001：能力受限扩展

状态：Accepted；阶段：P1 设计、P3 稳定。

验收：`ACC-P3-001`。

插件必须声明版本、能力、资源预算和输入输出契约；宿主默认拒绝未授予能力。不可信插件不得作为核心进程原生动态库加载。

### FR-AI-001：受控 AI 工具接口

状态：Accepted；阶段：P4。

验收：`ACC-P4-001`。

AI 必须通过强类型、最小权限、可审计的工具调用访问系统。模型输出不得绕过权限、人工审批、安全控制或确定性评测证据。

### FR-PROFILE-001：可扩展评测 Profile

状态：Accepted；阶段：P0 基础、P3 稳定。

验收：`ACC-P0-001`、`ACC-P3-002`。

协议必须允许在不改变核心 Submission/Evaluation/Artifact/Evidence 模型的前提下增加算法题之外的 Profile；未知 Profile 必须显式拒绝或能力协商，不得误判为算法题。
