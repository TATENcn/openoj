---
status: Proposed
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: Rust workspace and source layout
references:
  - overview.md
  - decisions/0001-rust-and-firecracker.md
---

# Workspace 与 crate 边界

以下是首个垂直切片的目标布局；创建 workspace 时可以通过 ADR 调整名称，但不得破坏依赖方向。

P0-A 已实现 `openoj-domain`、`openoj-protocol` 和 `openoj-application`。P0-B 增加 `openoj-storage` 和 `openoj-cli`：前者负责 PostgreSQL migration、兼容检查、持久化 transaction 和可靠 task/outbox，后者只组装 migration、提交和状态查询命令。P0-C 增加 `openoj-judge-protocol`、`openoj-judge-core`、`openoj-control-plane` 和 `openoj-judge-node`，交付 UDS gRPC Judge Control 契约、单并发 worker 与显式 development mock，形成真实双进程闭环。P0-D 增加 `openoj-guest-protocol`（guest↔host vsock 有界消息 codec)、`openoj-firecracker`（jailer/VMM/vsock/control-API 系统适配器）与 `openoj-guest-agent`（guest 内受限 vsock 命令 agent）。未实现组件不创建空 crate，每增加一个 crate 都必须同时交付其边界行为和测试。

```text
apps/
  openoj-cli/               # 已实现
  openoj-control-plane/     # 已实现：UDS Judge Control server 组装
  openoj-judge-node/        # 已实现：UDS client 与显式 development-mock worker 组装
crates/
  openoj-domain/            # 已实现
  openoj-guest-protocol/    # 已实现：guest↔host vsock 有界消息 codec
  openoj-protocol/          # 已实现
  openoj-judge-core/        # 已实现：transport-neutral 单节点 worker 与 development mock
  openoj-judge-protocol/    # 已实现：内部 Judge Control Protobuf/gRPC 契约
  openoj-application/       # 已实现
  openoj-storage/           # 已实现
  openoj-scheduler/         # 规划中
  openoj-evaluator/         # 规划中
  openoj-firecracker/       # 已实现：jailer/VMM/vsock/control-API 系统适配器
  openoj-plugin-host/       # 规划中
  openoj-observability/     # 规划中
guest/
  openoj-guest-agent/       # 已实现：guest 内受限 vsock 命令 agent
  openoj-guest-protocol/    # 已实现：guest↔host vsock codec（见上述 crates/）

# 规划中的进程（尚未创建空 crate）
apps/
  openoj-api/
web/
schemas/
infra/
```

## 依赖方向

```text
domain <- application <- apps
   ^           ^
   |           +-- scheduler / evaluator interfaces
   +-- protocol conversion boundaries

domain <- judge-protocol -> protocol

guest-protocol   # 与 domain/外部隔离的有界 vsock codec

firecracker <- judge-node
storage     <- application adapters
plugin-host <- application capability adapters
```

## 边界

- `openoj-domain` 只包含领域类型、规则和纯逻辑；不得依赖 async runtime、数据库、HTTP、Firecracker 或 UI。
- `openoj-protocol` 包含 canonical schema 对应类型和兼容转换；不得承载权限或业务决策。
- `openoj-judge-protocol` 包含内部 Judge Control `.proto`、生成绑定与有界 transport 转换；不得复制 canonical Evaluation 语义、承载授权或依赖数据库。
- `openoj-guest-protocol` 只包含 guest↔host vsock 消息类型与有界 frame codec；不承载权限、业务决策，不依赖 async runtime、数据库或 Firecracker。
- `openoj-judge-core` 包含单节点 worker 和显式 development mock executor；不得依赖 Tonic、SQLx、Firecracker 或宿主进程执行 API。
- `openoj-application` 编排用例并依赖 trait，不依赖具体数据库和 VMM 实现。
- `openoj-storage` 实现持久化、transaction、outbox 和 migration，不把数据库类型泄漏到 domain。
- `openoj-firecracker` 封装 jailer/VMM、vsock、磁盘、网络和回收；不得包含用户、竞赛或计分逻辑。
- `openoj-judge-node` 组合执行适配器，不能访问控制平面数据库超级权限。
- `openoj-guest-agent` 面向最小 guest 环境，只依赖 guest 所需协议，不依赖控制平面 crate。
- `openoj-plugin-host` 是能力 Broker；插件不能直接获取存储、网络或宿主句柄。
- `apps` 只负责进程组装、配置、生命周期和边界日志，不承载可复用领域实现。

## 规则

- workspace 直接依赖统一声明在根 `Cargo.toml`，版本和 feature 由 workspace 管理。
- 禁止循环依赖、复制类型绕开边界或通过 re-export 掩盖反向依赖。
- 平台 `unsafe` 必须收敛在最小系统适配模块，并由安全 API 封装。
- canonical schema 的生成代码不得手工修改。
- 建立 workspace 后应增加自动依赖方向检查。
- 当前依赖方向由 `python3 scripts/check-workspace.py` 检查；新增 crate 时必须在同一变更中更新本文和检查策略。
