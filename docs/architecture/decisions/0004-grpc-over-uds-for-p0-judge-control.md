---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0 control-plane to judge-node communication
references:
  - 0002-modular-control-and-execution-planes.md
  - ../overview.md
  - ../technology-stack.md
  - ../../security/trust-boundaries.md
  - ../../protocol/versioning.md
  - ../../superpowers/specs/2026-08-17-p0c-judge-control-design.md
---

# ADR-0004：P0 Judge Control 使用 gRPC over UDS

## 背景与需求

ADR-0002 已确定控制平面与 judge node 是独立进程，judge node 不读取控制平面数据库内部 Schema。P0-C 需要让两个进程实际完成任务领取、续租、取消和结果提交，同时保持 `FR-SCHED-001`、`NFR-REL-001`、`NFR-OPEN-001` 与信任边界要求。

当前只有进程内 `EvaluationStore` port。若直接把该 trait 或 PostgreSQL 表暴露给 judge node，将把数据库实现、服务端时钟和租约策略泄漏到执行平面；若只保留 fake/in-process adapter，又无法验证跨进程版本、消息上限、deadline、断线与滚动组合。

## 候选方案

### Rust trait 与进程内 adapter

依赖和实现成本最低，适合单元测试；但不形成独立进程边界，无法提供跨语言契约或覆盖传输失败。它保留为业务层测试 seam，不作为 P0-C 的交付终点。

### HTTP/JSON over loopback 或 UDS

可复用 Axum 与现有 canonical JSON，调试直观；但会过早把内部节点控制和未来公开 HTTP API 放进同一表示层。二进制 canonical payload、method deadline、生成 client/server 和未来双向/流式扩展需要额外自定义约定。

### Protobuf/gRPC over Unix Domain Socket

Protobuf 提供独立、可生成和可做兼容检查的内部契约；gRPC 提供 method、deadline、状态码和 client/server 边界。P0 只在本机 UDS 上监听，不开放 TCP；未来跨主机时可以保留 RPC service 并新增 TCP+mTLS transport。代价是引入 Tonic/Prost/protoc 构建链和一套独立于开放评测协议的版本矩阵。

## 决策

P0-C 选择 Protobuf/gRPC over UDS 作为控制平面到 judge node 的内部 Judge Control transport：

- service package 为 `openoj.judge.control.v0alpha1`，与开放评测协议、guest/host 协议和数据库 schema 分别版本化；
- UDS 是 P0-C 唯一启用的 transport，不创建 TCP listener；socket 父目录和文件权限默认仅部署身份可访问；
- gRPC message 不复制 `EvaluationRequest`/`EvaluationResult` 字段，使用 `bytes` 携带现有 canonical JSON，并在转换边界再次执行现有协议校验；
- node 请求不携带可信时间或任意 lease duration；控制平面拥有 clock、租约策略和状态转换；
- `node_id` 是关联身份，不是密码。P0-C 通过部署配置 allowlist 和 UDS 权限绑定允许节点；跨主机生产身份留给后续 TCP+mTLS 决策；
- judge node 不依赖 `openoj-storage`、SQLx 或数据库 schema，只依赖生成的 Judge Control client、领域/协议转换和 node runtime；
- development mock executor 必须显式配置，结果保持 `development_mock` 且 `production_eligible = false`。生产配置不得接受 mock executor。

本决策不稳定或发布公开 RPC；`v0alpha1` 允许显式破坏性演进，但 producer、consumer、fixture、版本矩阵和 migration 必须同一变更更新。

## 后果

- 新增 `tonic = 0.14.6`（MIT）、`prost = 0.14.4`（Apache-2.0）、`tonic-prost-build = 0.14.6`（MIT）、`protoc-bin-vendored = 3.2.0`（MIT）和已在 lockfile 中存在的 `tokio-stream = 0.1.19`（MIT）；所有直接依赖精确固定、关闭不需要的默认 feature 并记录供应链影响。
- 生成代码只由 canonical `.proto` 产生，不手工修改；CI 必须证明重新生成没有漂移。
- UDS 权限只适用于 P0 单机部署，不能作为跨主机或多租户节点认证结论。
- RPC 边界可以替换 transport，但 application/store port、canonical Evaluation 语义和 Firecracker adapter 不依赖 Tonic 类型。
- P0-C 会增加真实进程和故障模式，需要覆盖 deadline、断线、重复请求、优雅停止和 socket 清理。

## 迁移与回退

P0-C 新增独立 RPC，不替换现有 CLI。数据库只通过新增前向 v2 migration 增加 capability dispatch 与 claim replay 元数据；已发布 v1 migration 不改写。

部署按“migration → control RPC server → development judge node”顺序进行。RPC server 未就绪或版本不兼容时 node fail closed 且不领取任务。回退触发包括错误投递、同一 Attempt 多个有效租约、版本拒绝失效或身份绕过：停止 judge RPC 写入，保留数据库和任务行，回滚到最后一个理解 schema v2 的构建或在无新写入时从迁移前备份恢复。不得篡改 schema version 或自动删除终态。

未来新增 TCP+mTLS 时必须补充节点身份、证书轮换、网络策略、滚动升级矩阵和安全评审；不得仅把 UDS listener 改为 `0.0.0.0`。

## 验证

- `.proto` lint/descriptor 与生成代码可复现；未知 service version 和 capability 明确拒绝。
- 真实 PostgreSQL 18、真实 UDS、两个真实进程完成 negotiate、claim、renew/cancel、development-mock result submit 闭环。
- 注入响应丢失、重复 claim、重复 result、过期 lease、错误 token、错误 node、超限 canonical payload 和进程终止。
- workspace fmt/check/Clippy/tests、依赖方向、文档、cargo-deny 和 dependency feature 审计通过。
- TCP/mTLS、跨主机、Firecracker、用户代码、安全隔离和性能保持 `Unverified`。

维护者已于 2026-08-17 批准本 ADR；后续改变 transport、节点身份或信任边界必须新增替代 ADR。
