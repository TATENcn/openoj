---
status: Validated
owners: OpenOJ control-plane, execution-plane, operations and security maintainers
last_reviewed: 2026-08-25
applies_to: development-only algorithm-c real-microVM failure persistence at commit 752880c6f701256a86449a2e57be9592ba969703
references:
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../operations/algorithm-c-development-runtime.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../security/threat-model.md
  - ../security/trust-boundaries.md
  - 2026-08-25-algorithm-c-process-e2e.md
  - 2026-08-25-algorithm-c-runtime-kvm.md
---

# algorithm-c 真实 microVM 失败终态验证（2026-08-25）

## 范围与结论

本记录验证 commit `752880c6f701256a86449a2e57be9592ba969703`：固定编译错误与无限循环
C Artifact 经过真实 CLI、PostgreSQL、control-plane UDS、judge-node、Firecracker/vsock 和 guest
agent 后，分别持久化为 `CompileError` 与 `TimeLimitExceeded`。失败 stage 保留有界诊断与内容
寻址 Evidence，后续 stage 为 `Skipped`，VMM 与 socket 完成回收。

该证据扩展 `2026-08-25-algorithm-c-process-e2e.md` 的 Accepted 主路径，为
`FR-EVAL-001`、`FR-JUDGE-002/003`、`FR-RESULT-001`、`ACC-P0-001/004/009/010/016` 与
`RISK-EXEC-001` 提供 development-only 负路径的局部验证。它不实现正式对象存储、任意用户
Artifact、取消/崩溃竞态、恶意资源耗尽矩阵或 production isolation。

## 固定 case 与断言

三个 case 使用同一参数化进程编排，每个 case 创建独立数据库、私有 UDS 和全新 microVM：

| case | source SHA-256 | 持久化 Verdict | 终止 stage |
|---|---|---|---|
| Accepted | `c2329bffd207edd619d4ca7c7cdd872b76374e1a50421f705020671ca1f85556` | `Accepted` | Check 成功 |
| compile error | `45c508a05870369dd65fb951ee804127925b454fa0078ac6e37f84f5a1e3fa06` | `CompileError` | Build 失败 |
| infinite loop | `84950edbf9514ebef845181f9f296bf2a71be3032f0d1d27b28b93fe1fd57f54` | `TimeLimitExceeded` | Run 失败 |

失败 case 共同断言：结果 provenance 为 Firecracker、`production_eligible=false`、node ID 为
`judge_node_fc_01`；终止 stage 状态为 `Failed`，诊断非空且 Evidence 引用内容摘要 Artifact；
之后的 canonical stage 全部为 `Skipped`；judge-node 停止后 API/vsock 路径不存在。

## 环境与产物

- Arch Linux x86_64，kernel `7.1.8-zen1-3-zen`；AMD Ryzen 9 9950X3D，8 个可见 CPU，
  15 GiB 内存；VMware 暴露 AMD-V。
- `/dev/kvm` 为 `crw-rw-rw- root:kvm`，KVM API `12`；Firecracker `1.16.1`；Rust/Cargo
  `1.97.1`。
- 一次性 PostgreSQL 18 Alpine 仅绑定 `127.0.0.1:55437`，数据位于 tmpfs，测试后已删除；
  image digest 为
  `postgres@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2`。

| 产物 | SHA-256 |
|---|---|
| kernel | `882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef` |
| rootfs | `e9b2ca2f060161b01ccaafc20fd56e246def6b41011b4183d5c72f40b50f6f9c` |
| guest agent | `e775d7d0fe71b8980a58ad02b2a9b8ac1195d6b32fc745db659750e46373cae2` |
| runtime manifest | `c5ab7fb51144f39837e47799c2d7f1032f485b50a99337cb8fb84b41e3a12ea3` |
| SPDX SBOM | `aee7b475b81add5b58f497accdad5d4ef10a5ba8ad06e89007e2427e904ecddb` |

本分支堆叠在真实进程闭环 PR 上，与 rootfs 可复现构建 PR 并列；测试从已校验 manifest 动态读取
Runtime 摘要，不硬编码 rootfs。上表只描述本次实际验证产物，不替代可复现构建的独立证据。

## 命令与结果

以下命令通过：

```text
OPENOJ_RUNTIME_OFFLINE=1 OPENOJ_RUNTIME_CACHE=/tmp/openoj-algorithm-c-cache bash infra/runtime-images/algorithm-c/provision.sh
(cd infra/runtime-images/algorithm-c/out && sha256sum --check --strict manifest.sha256)
OPENOJ_REQUIRE_KVM=1 OPENOJ_TEST_DATABASE_URL=<PostgreSQL 18> cargo test -p openoj-control-plane --test process_e2e real_microvm_ --locked -- --nocapture --test-threads=1
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
# 同时设置 DATABASE_URL、OPENOJ_TEST_DATABASE_URL 与 OPENOJ_REQUIRE_KVM=1：
cargo test --workspace --all-targets --locked
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
# 用带 PyYAML 的隔离解释器运行四个适用 Skill 的 quick_validate.py
```

聚焦的三个真实 microVM 进程 case 串行运行 25.18 秒，3/3 通过。完整 workspace 共 151 个测试
通过、0 失败；其中 8 个 process E2E 在 13.16 秒通过，严格 runtime KVM smoke 在 1.87 秒通过，
17 个 PostgreSQL control-spine 测试通过。`cargo deny` 只有既有 duplicate-version 警告。

Clippy 首次拒绝 121 行的测试 helper；将持久化结果断言拆分后，在未添加 allow、未降低 lint 的
情况下通过。测试后无 Firecracker 进程残留；临时 PostgreSQL 已删除，既有离线 cache 保留。

精简原始证据位于 `artifacts/2026-08-25-algorithm-c-process-failures.txt`，SHA-256 为
`34def7013e0ffeaf1d80f0161a8905755f8aabc96ab548021019275d1b558b9a`。

## 安全影响与未验证项

本变更只扩展测试与验证证据，不修改协议、schema、migration、生产路径、网络、设备、凭证、
argv 或信任边界。失败 Verdict 继续由宿主根据有界 guest stage output 形成，guest 不自行声明
最终结果；production profile 继续 fail closed。

仍未验证：正式对象存储 Artifact 正文与短期授权、HTTP/API 与身份审计、取消/强杀/租约过期
期间的真实 microVM 竞态、CPU/内存/process/disk/I/O/无限输出与并发 guest 宿主存活矩阵、
production jailer/cgroup/namespace/seccomp/watchdog、第三方许可证人工复核及 release 性能。
