---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: dependencies and supply chain
references:
  - ../architecture/technology-stack.md
  - ../governance/licensing.md
  - ../security/threat-model.md
---

# 依赖与供应链规范

- 直接依赖统一声明并由 lockfile 固定；应用和工具使用 `--locked` 门禁。
- 关闭不需要的 default features，解释大型、原生、网络、密码学和 `unsafe` 依赖的 feature。
- 禁止 Git 分支、未校验下载地址或来源不明二进制成为生产依赖。
- 新依赖记录用途、维护状态、许可证、来源、已知 advisory、替代方案和移除成本。
- 密码学、签名、压缩、解析和虚拟化不得自创未经审查的基础算法。
- kernel、rootfs、编译器和 runtime image 与普通库同样纳入来源、摘要、SBOM 和漏洞响应。
- 依赖升级独立成变更，保留 lock diff，运行关键 API、协议和安全回归。
- 发布前的允许许可证、DCO/CLA 和再分发规则以 Accepted 许可证 ADR 为准；当前许可未决是发布阻塞项。

## P0-A 直接依赖记录

| 依赖 | 固定版本 | 用途与 feature | 来源与维护状态 | 许可证 | 替代与移除成本 |
|---|---:|---|---|---|---|
| `serde` | 1.0.229 | 仅协议边界 derive；默认 `std` | crates.io，serde-rs 官方仓库，当前维护 | MIT OR Apache-2.0 | 可替换但会影响全部 wire 类型 |
| `serde_json` | 1.0.151 | 有界 JSON 解析；不启用 `unbounded_depth` | crates.io，serde-rs 官方仓库，当前维护 | MIT OR Apache-2.0 | 替换会影响 canonical JSON 行为和 fixture |
| `typify` | 0.7.0 | 从 JSON Schema 生成 Rust wire 类型；默认 macro | crates.io，Oxide Computer 官方仓库，当前维护 | Apache-2.0 | alpha 阶段可替换，需重跑生成和 conformance |
| `regress` | 0.11.1 | `typify` 生成的字符串约束实现 | crates.io，随生成代码显式固定 | MIT | 移除要求改变生成器或 schema 约束实现 |
| `jsonschema` | 0.49.9 | canonical schema 运行时验证；关闭全部 default feature，禁止隐式 HTTP/文件解析 | crates.io，Stranger6667 官方仓库，当前维护 | MIT | 移除前必须证明生成类型覆盖所有 schema 与跨字段限制 |

以上依赖只用于研究和原型，不表示已经完成正式分发许可证审批。维护者已明确将完整 license gate 延后到许可证策略 Accepted 之后；当前 CI 只强制 advisory、ban 和 source 检查，正式发布仍由许可证未决状态阻塞。实际依赖检查结果记录在 `../validation/2026-08-16-p0-evaluation-kernel.md`。

## P0-B 直接依赖记录

| 依赖 | 固定版本 | 用途与 feature | 来源与维护状态 | 许可证 | 替代与移除成本 |
|---|---:|---|---|---|---|
| `sqlx` | 0.9.0 | PostgreSQL pool/query/migration；关闭 default features，生产只启用 `postgres`、`runtime-tokio`、`migrate`、`tls-rustls-ring-native-roots`；测试额外启用 `macros` 以创建隔离数据库，不使用在线 query macro | crates.io，launchbadge/sqlx 官方仓库，当前维护；MSRV 1.94 | MIT OR Apache-2.0 | application port 隔离替换面；替换需要重写 adapter、migration 与数据库测试，成本中等 |
| `tokio` | 1.53.1 | SQLx 已接受运行时；关闭 default features，测试启用 `macros`、`rt-multi-thread`，CLI 按实际 I/O 再增加最小 feature | crates.io，tokio-rs 官方仓库，当前维护；MSRV 1.71 | MIT | 已由技术栈接受；移除会影响数据库和未来进程生命周期，成本高 |

SQLx migration SQL 由 `include_str!` 嵌入，并由 `Migrator::with_migrations` 在运行时计算和校验 checksum，因此 production feature 不需要宏依赖。`Cargo.lock` 仍包含测试宏的传递依赖；它们不进入 `cargo build -p openoj-storage --lib` 的 production feature 图。P0-B 的许可证结论与 P0-A 相同：研究和原型允许继续，正式分发继续被许可证策略未决状态阻塞。
