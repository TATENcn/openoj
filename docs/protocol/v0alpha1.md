---
status: Implemented
owners: OpenOJ protocol maintainers
last_reviewed: 2026-08-16
applies_to: v0alpha1 EvaluationRequest and EvaluationResult
references:
  - README.md
  - versioning.md
  - ../requirements/acceptance.md
  - ../security/threat-model.md
---

# Open Evaluation Protocol v0alpha1

`v0alpha1` 实现首个算法批处理 Profile 的 `EvaluationRequest` 和 `EvaluationResult`。canonical schema 位于 `schemas/openoj/v0alpha1/open-evaluation.schema.json`；Rust wire 类型由该文件生成，不得手工复制或修改生成类型。

本版本是 alpha。允许通过受控变更破坏兼容，但 schema、生成类型、显式领域转换、fixture、语义检查和本文必须在同一变更更新。

## 支持矩阵

| Producer | Consumer | 状态 |
|---|---|---|
| `v0alpha1` | `v0alpha1` | Implemented |
| 其他版本 | `v0alpha1` | 显式拒绝 |
| `v0alpha1` | 其他版本 | Unverified，不声明兼容 |

当前只支持 `algorithm_batch` Profile 和以下固定阶段顺序：

```text
prepare -> build -> run -> check -> aggregate
```

未知 Profile、阶段、Verdict、状态、字段或 schema 版本不得映射到成功语义。请求和结果分别限制为 256 KiB 和 1 MiB；字段、集合、诊断、Artifact 和资源计数还受 schema 与领域硬上限约束。

## 请求语义

请求明确携带 Request、Evaluation、Attempt、Problem Version、Submission 和 Runtime 的不同身份。`attempt_number` 从 1 开始；重试使用新 Attempt，不改变 Submission 或 Evaluation 身份。

`idempotency_key` 是调用方稳定重放键。当前内存内核只验证并保留它，持久化幂等在后续 storage 切片实现。

`required_capabilities` 只声明执行需求，不授予权限。executor 缺少任何能力时请求在执行阶段前被拒绝。P0-A 唯一 fixture 需要 `algorithm.batch`。

`policy.network` 当前只接受 `denied`。这表示默认无网络的协议事实，不表示 mock executor 已验证真实网络隔离。

Artifact 只携带有界元数据、敏感性和小写 SHA-256 内容摘要，不在消息中嵌入源码或其他无限内容。Artifact 位置、短期授权和摘要下载验证由后续 storage/judge-node 边界实现。

## 结果语义

结果必须保留请求、Evaluation、Attempt、Problem Version、Submission 和 Runtime 来源，并按 canonical 顺序包含五个阶段报告。每个阶段包含有界资源摘要、诊断和 Evidence 引用。

终态组合按以下规则 fail closed：

- `completed` 不得与 `cancelled` 或 `system_error` Verdict 组合。
- `accepted` 和 `wrong_answer` 要求五个阶段全部成功。
- `compile_error` 只允许 build 失败；运行类限制和错误只允许 run 失败；后续阶段必须 `skipped`。
- `failed` 只对应平台 `system_error`；`cancelled` 只对应 `cancelled` Verdict；两者都要求零得分、一个终止阶段、成功的前置阶段和跳过的后续阶段。
- mock provenance 的 `production_eligible` 必须为 `false`；生产可用来源必须包含 node identity。

诊断错误不会回显原始消息值或用户内容。解析和 schema 错误只返回稳定分类，详细输入不得写入边界日志。

## 生成与验证

`openoj-protocol` 使用固定版本的 `typify` 从 canonical schema 生成 wire 类型，并使用关闭外部 HTTP/文件解析能力的 `jsonschema` 进行运行时验证。边界先检查消息字节数，再进行有类型解析和 schema/跨字段语义验证，最后显式转换为领域对象。

conformance fixture 位于 `schemas/openoj/v0alpha1/fixtures/`。当前测试覆盖 schema 元验证、正向往返、重复字段、未知字段/Profile、超限消息、非法终态组合和伪造 mock 生产来源。

## 当前非目标

- HTTP API、SDK 和持久化消息兼容。
- control-plane/judge-node 或 guest/host 传输。
- Firecracker、Runtime image 或 Artifact 下载。
- 真实 checker、计分、资源计量和取消竞态。
- 第二个 Profile 或稳定协议承诺。
