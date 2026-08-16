---
status: Implemented
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: P0-A evaluation kernel working tree
references:
  - README.md
  - ../protocol/v0alpha1.md
  - ../requirements/acceptance.md
  - ../security/threat-model.md
  - ../development/dependencies.md
---

# P0-A 评测内核验证记录（2026-08-16）

## 范围与修订

- 范围：`FR-SUBMISSION-001`、`FR-EVAL-001`、`FR-RESULT-001`、`FR-PROFILE-001` 的首个内存实现，以及 `ACC-P0-012`、`ACC-P0-013`、`ACC-P0-016` 的早期机械覆盖。
- 修订：分支 `codex/p0-evaluation-kernel` 的未提交工作区，基于 `f798729f3f89d37a8ed22e2fe652063a419029a6`。建立不可变 commit 并通过远端 CI 前不得升级为 `Validated`。
- 环境：Arch Linux，Linux `7.1.8-zen1-3-zen`，x86_64，8 vCPU（AMD Ryzen 9 9950X3D，VMware full virtualization），15 GiB RAM；`/dev/kvm` 不存在。
- 工具：Rust/Cargo 1.97.1、cargo-deny 0.20.2、Python 3.14.7、Bash、ripgrep。
- 构建：debug 单元、集成和 conformance 测试；不包含 release 性能构建。
- Firecracker/kernel/rootfs/guest agent/Runtime image：未实现、未运行、无摘要。
- 输入：两个仓库自有最小 JSON fixture，无真实用户源码、秘密或受限制题目。
- 原始摘要：`artifacts/2026-08-16-p0-evaluation-kernel.txt`，SHA-256 `8ad771ce470187c614186a58dd45566c41600f46a83cefdb9bb73baa13522c91`。

## 需求追踪

| 范围 | 实现边界 | 证据 | 当前结论 |
|---|---|---|---|
| `ACC-P0-012` | 不同 ID newtype、Problem Version/Submission/Evaluation/Attempt/Runtime 引用 | domain 单元测试、request fixture 往返 | Implemented early slice；Rejudge 和持久化未实现 |
| `ACC-P0-013` | schema-first `v0alpha1`、生成 wire 类型、显式 domain 转换、结果跨字段检查 | schema 元验证、正向往返、未知字段/Profile、重复字段、非法终态和伪造来源测试 | Implemented for request/result subset；非稳定协议 |
| `ACC-P0-016` | 请求/结果字节上限、集合/字符串/资源硬上限、默认无网络策略 | 超限消息、资源边界和 capability 拒绝测试 | Implemented only for in-memory protocol/domain boundary；无队列、I/O 或 executor 资源证明 |
| `THR-PROTOCOL-001` / `CTL-PARSE-001` / `CTL-VERSION-001` / `CTL-STATE-001` | 解析前字节限制、未知语义 fail closed、canonical stage/terminal semantics | protocol conformance tests | Implemented for `v0alpha1` request/result subset |
| `THR-SUPPLY-001` | 固定工具链、精确直接依赖、lockfile、关闭 schema 外部解析 feature | Cargo locked gates、dependency inventory、cargo-deny 子门禁 | 部分实现；正式许可证策略阻塞 |

本记录不声明完整通过任何 P0 验收项。`ACC-P0-001` 所需 API/CLI、judge node、Firecracker guest、真实编译/运行/check 和持久化结果均不存在。

## 自动检查

以下命令通过：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
cargo deny --locked check advisories bans sources
git diff --check
```

测试共 16 项：application 单元测试 4 项、walking-skeleton 集成测试 1 项、domain 单元测试 6 项、protocol/conformance 测试 5 项，全部通过。

完整 `cargo deny --locked check` 失败：advisory、ban 和 source 通过，license 因仓库没有 Accepted 许可证允许列表而失败。维护者决定原型阶段暂不处理许可证允许列表，因此当前机械门禁只执行通过的三个子检查；完整 license gate 已预留，并继续阻塞正式发布和产物分发。

## 安全与兼容结论

- schema 是 wire 类型唯一来源；domain 不依赖 Serde、数据库、HTTP 或执行器。
- `jsonschema` 禁用默认 HTTP/文件解析 feature，canonical schema 只使用内部引用。
- 请求缺少能力、包含未知 Profile/字段/版本、重复字段、非法 stage 顺序或超过字节/集合/资源上限时显式拒绝。
- 结果拒绝未知或矛盾终态、错误 stage 顺序以及伪造为 production-eligible 的 mock provenance。
- mock executor 只存在于测试，不能运行用户命令，结果始终标记 `development_mock` 且非生产可用。
- 当前只声明 `v0alpha1` producer/consumer 同版本组合；无滚动升级、旧版本或第二实现兼容结论。

## 未验证与阻塞

- GitHub Actions：分支未提交、未推送，文档和 Rust workspace workflow 尚未在本分支远端运行。
- 许可证：平台许可证、依赖允许列表、DCO/CLA 和正式分发权利等待人工决策。
- KVM/Firecracker：当前环境无 `/dev/kvm`，且执行平面尚未实现。
- 真实安全：无恶意 workload、宿主存活、资源回收、网络隔离、秘密隔离或 guest/host 协议证据。
- 可靠性：幂等键仅进入领域对象，尚无数据库、重放、租约、崩溃恢复或取消竞态验证。
- 性能：未运行 release benchmark，不作吞吐或延迟声明。
