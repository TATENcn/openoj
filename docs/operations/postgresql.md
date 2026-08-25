---
status: Implemented
owners: OpenOJ operations maintainers
last_reviewed: 2026-08-17
applies_to: P0-B/P0-C PostgreSQL control spine
references:
  - deployment-profiles.md
  - ../architecture/overview.md
  - ../architecture/risks.md
  - ../development/testing.md
  - ../superpowers/specs/2026-08-17-p0b-durable-control-spine-design.md
  - ../superpowers/specs/2026-08-17-p0c-judge-control-design.md
---

# PostgreSQL 运维契约

P0-B/P0-C 的 CI 目标是 PostgreSQL 18 隔离测试实例；P0-C 本地已在一次临时 PostgreSQL 18 容器运行 migration 与 storage 回归，但这不构成生产高可用、性能或灾难恢复证明。数据库保存控制状态和 canonical 请求/结果，不保存 Artifact 正文。

## 配置与秘密

- 进程只从 `OPENOJ_DATABASE_URL` 读取连接串；连接串必须由部署环境的秘密机制注入，不得写入仓库、命令输出、日志或指标标签。
- `OPENOJ_DATABASE_MAX_CONNECTIONS` 可选，默认 `4`，只接受 `1..=64`。每个进程的总连接预算必须纳入数据库容量规划。
- 应用账号只获得目标 schema 所需权限。测试账号、开发账号和生产账号不得复用；judge node 和 guest 永远不得获得该连接串。
- TLS 验证使用系统原生根证书；生产连接是否强制 TLS、证书与 PostgreSQL 发行版组合仍为 `Unverified`，部署前必须专项验证。

## 启动与迁移

迁移是显式运维步骤，不由 `submit`、`status` 或 `cancel` 隐式执行：

```bash
OPENOJ_DATABASE_URL='<secret>' \
  OPENOJ_DATABASE_MAX_CONNECTIONS=4 \
  cargo run --locked -p openoj-cli -- migrate
```

1. 在维护窗口确认目标、备份状态、可恢复点和当前 `openoj_schema_metadata`。
2. 先对备份恢复出的隔离实例运行同一二进制和 migration。
3. 停止旧版本新写入，执行 `migrate`，确认输出 `migrated schema 2`。
4. 再启动 `submit`/`status`/`cancel` 路径。缺表、损坏或高于应用支持版本的 schema 必须 fail closed；不得跳过兼容检查。

已共享 migration 只能新增，不能改写。当前 v2 以前向方式从 v1 的 canonical UTF-8 JSON `request_payload` 严格回填排序、去重的 task `required_capabilities`，并加入未来 P0-C claim operation ID 的索引位；非法 UTF-8、非 JSON、缺失/空/重复/超限或非法 capability 会使 migration 整体失败。升级必须交付旧版 fixture、重复执行、失败回滚和数据保留测试。

## Development 取消命令

持有 development 数据库凭证的受信 operator 可以发起幂等取消：

```bash
OPENOJ_DATABASE_URL='<secret>' \
  cargo run --locked -p openoj-cli -- \
  cancel <evaluation-id> <caller-stable-idempotency-key>
```

- 同一取消意图的超时重试必须复用相同幂等键；相同键与相同目标返回同一终态快照，不同终态或
  键冲突 fail closed。
- CLI 不接受 result 文件、Verdict、Score、Attempt、provenance、阶段或证据参数。storage 在同一
  `FOR UPDATE` transaction 内读取当前 Attempt 的 canonical request，生成
  `DevelopmentMock`、`production_eligible=false`、无 node provenance 的取消结果并提交
  first-terminal-wins 写入；该 provenance 表示控制面决策，不伪装成 guest/Firecracker 证据。
- 当前命令是直连 PostgreSQL 的 development 运维入口，不含主体认证、scope、审批或结构化审计，
  不得向不可信用户暴露，也不构成 production 管理 API。正式服务入口仍需满足 `ACC-P0-011/019`。

## 备份、回滚与事故处理

- migration 前使用受支持的 PostgreSQL 物理或逻辑备份建立可验证恢复点，并在隔离实例实际恢复；只生成备份文件不构成恢复证明。
- v1→v2 migration 为加法变更。应用异常时停止新写入并回滚到最后一个理解 schema v2 的应用构建，保留 schema 和全部行用于审计；不得自动降级 schema、删除表、改写 migration 或清空终态结果。
- schema 版本过新只能部署兼容应用或恢复到事故前独立实例，不能篡改 `schema_version` 绕过拒绝。
- 发现重复终态、并发有效租约或引用不一致时，先停止创建、领取、重试、结果和取消写入；保存数据库快照与脱敏日志。任何数据修复必须单独设计、测试、审核并保留审计证据。
- 连接不可用时 CLI 只返回稳定脱敏错误。排障从服务健康、账号权限、TLS、连接上限和 schema 版本入手，不在工单或聊天中粘贴连接串、canonical payload、源码或隐藏测试数据。

## 当前未验证

- 生产 PostgreSQL 版本/扩展矩阵、复制、故障转移、时间点恢复和长期数据增长。
- 大规模竞争、连接池容量、vacuum、索引膨胀、吞吐和尾延迟。
- 真实进程被强杀、主机重启、网络分区和跨版本滚动升级。
- 数据修复工具、自动备份与恢复演练。
- production 取消 API 的认证、授权、审批、限流和结构化审计。
