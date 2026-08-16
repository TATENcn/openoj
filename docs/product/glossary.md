---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: repository terminology
references:
  - vision.md
  - ../protocol/README.md
---

# 术语表

| 术语 | 定义 |
|---|---|
| Problem | 用户可发现的题目身份，不包含可变实现细节。 |
| Problem Version | 不可变的题面、数据、限制、评测计划和内容摘要集合。 |
| Submission | 用户提交的候选产物及其元数据；它本身不等同于一次执行。 |
| Evaluation | 针对确定 Problem Version、Submission、Runtime 和 Policy 的一次逻辑评测。 |
| Attempt | Evaluation 的一次可重试物理执行；重试不得创建新的逻辑提交。 |
| Stage | 评测计划中的有类型步骤，例如 build、run、check、aggregate。 |
| Artifact | 按内容摘要寻址的输入或输出，例如源码、测试数据、日志或报告。 |
| Evidence | 支撑结果的结构化事实或 Artifact 引用。 |
| Verdict | 离散判定，例如 Accepted、Wrong Answer、Time Limit Exceeded。 |
| Score | 由 evaluator 产生并按规则聚合的数值或结构化评分。 |
| Runtime | 被版本和摘要固定的编译器、解释器、依赖与执行环境描述。 |
| Evaluator | 消费输入和证据并产生判断、分数或新证据的组件。 |
| Executor | 在受限环境中执行某个 Stage 的组件。 |
| Judge Node | 管理执行资源、microVM 生命周期和本地回收的工作节点。 |
| Guest Agent | 运行在 microVM 内，执行经过验证的受限命令协议。 |
| Control Plane | 管理领域对象、API、策略、调度意图和用户可见状态的组件集合。 |
| Execution Plane | 执行任务并采集证据的 judge node、VMM 和 guest 组件集合。 |
| Rejudge | 使用新 Problem Version、Runtime 或 Policy 创建新的 Evaluation，保留旧结果。 |
| Capability | 插件、节点或 Agent 被明确授予并可协商的有限能力。 |

协议和代码必须优先使用这些术语，不得用 `job`、`run`、`result` 等模糊词同时表示多个领域对象。
