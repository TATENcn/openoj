---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: technology choices
references:
  - decisions/0001-rust-and-firecracker.md
  - overview.md
  - ../governance/licensing.md
---

# 技术栈基线

## 已接受方向

| 领域 | 选择 | 约束 |
|---|---|---|
| 核心语言 | Rust 2024，固定稳定工具链 | 禁止依赖未固定 nightly；版本由 `rust-toolchain.toml` 固定 |
| 异步运行时 | Tokio | 所有队列、并发、超时和取消必须有界 |
| HTTP API | Axum | 只在 API 边界使用框架类型 |
| 内部 RPC | Tonic/Protobuf 候选 | 与公开评测语义通过显式转换隔离 |
| 序列化 | Serde | 不可信输入设置深度、大小和集合上限 |
| 数据库 | PostgreSQL + SQLx | 编译期查询不是绕过 migration/兼容测试的理由 |
| 大型产物 | S3 兼容对象存储 | 内容寻址、完整性、敏感级别和保留策略 |
| 可观测性 | tracing + OpenTelemetry | 低基数字段、端到端关联和敏感数据最小化 |
| 不可信执行 | Firecracker + jailer + KVM | 结合 cgroup、namespace、seccomp、最小权限和 watchdog |
| guest 通信 | 版本化 vsock 协议 | 不提供通用 shell；有大小、取消和超时约束 |
| 前端 | TypeScript + React | API 生成客户端；不把权限仅放在前端 |
| 插件 | Wasm Component/WASI 方向 | 固定受支持版本，所有宿主能力通过 Broker |

具体 crate/npm 版本在 workspace 建立时固定，并通过 lockfile、依赖策略和供应链记录管理。

## 暂不引入

- 在 P0 证据证明需要前，不引入 Kafka、NATS 或复杂工作流平台。
- 不把 Kubernetes 作为 Firecracker 生产运行前提。
- 不使用宿主 Docker 容器作为生产不可信代码的最终隔离边界。
- 不在核心进程加载第三方原生动态库。
- 不使用通用远程 shell 作为 guest agent 协议。

## 新增关键依赖

引入或替换关键依赖时必须记录用途、维护状态、feature、许可证、供应链来源、安全影响、替代方案和移除成本。改变基础设施类别、公开协议或安全边界时新增 ADR。
