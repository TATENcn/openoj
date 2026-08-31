---
status: Validated
owners: OpenOJ operations, execution-plane and security maintainers
last_reviewed: 2026-08-25
applies_to: same-host algorithm-c runtime repeated-build reproducibility at commit 17f178a6c7413a65c937430d0f638a9583d03936
references:
  - ../operations/algorithm-c-development-runtime.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../requirements/non-functional.md
  - ../security/threat-model.md
  - ../architecture/risks.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - 2026-08-25-algorithm-c-runtime-kvm.md
---

# algorithm-c rootfs 可复现构建验证（2026-08-25）

## 范围与结论

本记录验证 commit `17f178a6c7413a65c937430d0f638a9583d03936`：相同 checkout、锁定输入、
离线 cache 与主机工具版本连续执行两次完整 `algorithm-c` 供应流程，kernel、rootfs、guest
agent、manifest、checksum manifest 和 SBOM 全部逐字节一致；新 rootfs 随后在真实 KVM 上通过
成功编译运行、编译失败、运行超时和幂等回收 smoke。

这为 `FR-RUNTIME-001`、`ACC-P0-004/008`、`THR-SUPPLY-001`、`RISK-REPRO-001` 与
`RISK-SUPPLY-001` 提供开发运行时的局部证据。它不证明跨发行版或工具版本可复现，不关闭完整
判题确定性、production isolation、签名/来源证明、许可证人工复核或正式分发验收。

## RED → GREEN

修复前，两次锁定输入构建的 guest agent 与 kernel 摘要相同，但 rootfs 分别为
`15fed4a…` 和 `7a7f28d…`。`dumpe2fs` 首先定位到随机 `Directory Hash Seed`。只固定该 seed
后，rootfs 仍分别为 `098b92d…` 和 `86f6cca…`；`debugfs` 进一步确认 `mke2fs -d <目录>`
把 staging inode 的真实 `ctime` 带入镜像，两次相差 13 秒。

修复后的供应流程先创建规范化 POSIX tar：路径排序，固定 `mtime` 与 uid/gid，删除 pax
`atime/ctime`，拒绝 ACL、SELinux 与 xattr；随后 `mke2fs` 从 tar 导入，并显式固定 filesystem
UUID、directory hash seed 与 `E2FSPROGS_FAKE_TIME`。仓库化的 `verify-reproducible.sh` 连续构建
两次并递归比较整个输出目录，任何差异都会失败。

两次 rootfs SHA-256 均为：

```text
26f7a7a0375bde0bab2609eec701403d5f4bae32156995f0f7720ce0138faf80
```

## 环境与产物

- Arch Linux x86_64，kernel `7.1.8-zen1-3-zen`；AMD Ryzen 9 9950X3D，8 个可见 CPU，
  15 GiB 内存；VMware 暴露 AMD-V。
- `/dev/kvm` 为 `crw-rw-rw- root:kvm`，KVM API version `12`；Firecracker `1.16.1`。
- Rust/Cargo `1.97.1`；`mke2fs/libext2fs 1.47.4`；GNU tar `1.35`。
- 供应使用已校验的 `/tmp/openoj-algorithm-c-cache` 离线 cache；`sources.lock.json` SHA-256 为
  `322668c764a4b9fe325c079a3fa10eb552fb34ef57b2dee53b89109a5fcc880c`。
- workspace 测试使用一次性 PostgreSQL 18 Alpine 容器，镜像摘要为
  `postgres@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2`；
  仅绑定 `127.0.0.1:55436`，数据目录为 tmpfs，测试后已删除。

| 产物 | SHA-256 |
|---|---|
| kernel | `882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef` |
| rootfs | `26f7a7a0375bde0bab2609eec701403d5f4bae32156995f0f7720ce0138faf80` |
| guest agent | `e775d7d0fe71b8980a58ad02b2a9b8ac1195d6b32fc745db659750e46373cae2` |
| C toolchain source lock | `5d60f2080509113459fe14dca82594c87016f5a869c26351d9d6b2af55bb51c6` |
| manifest | `d0c5a96d736204b3ae0cbcf057c2003dce42726e0d3271f24fdcf0cccfc0f81e` |
| SPDX SBOM | `aee7b475b81add5b58f497accdad5d4ef10a5ba8ad06e89007e2427e904ecddb` |

rootfs 的 filesystem UUID 与 directory hash seed 均为
`8d617bd3-5336-4aec-926a-1d5c12d7f009`，source date epoch 为 `1711929600`。`debugfs` 确认
`/work` 仍为 `0700 root:root`；本地产物保持 Git ignored，不构成发布制品。

## 命令与结果

以下命令通过：

```text
bash infra/runtime-images/algorithm-c/check.sh
OPENOJ_RUNTIME_OFFLINE=1 OPENOJ_RUNTIME_CACHE=/tmp/openoj-algorithm-c-cache bash infra/runtime-images/algorithm-c/verify-reproducible.sh
(cd infra/runtime-images/algorithm-c/out && sha256sum --check --strict manifest.sha256)
OPENOJ_REQUIRE_KVM=1 cargo test -p openoj-firecracker --test boot --locked -- --nocapture
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
# 在上述一次性 PostgreSQL 上同时设置 DATABASE_URL、OPENOJ_TEST_DATABASE_URL 与 OPENOJ_REQUIRE_KVM=1：
cargo test --workspace --all-targets --locked
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
# 用带 PyYAML 的隔离解释器运行四个适用 Skill 的 quick_validate.py
```

严格 KVM 集成测试 1/1 通过（1.80 秒）。完整 workspace 共 145 个测试通过，包括 17 个
PostgreSQL 测试、control-plane/judge-node 跨进程测试与真实 KVM smoke。`cargo deny` 只有仓库
既有 duplicate-version 警告；其余门禁和四个 Skill validator 均通过。测试后无 Firecracker
进程残留；本任务的临时 PostgreSQL 与诊断目录已删除，既有离线 cache 按预期保留。

精简原始证据位于 `artifacts/2026-08-25-algorithm-c-rootfs-reproducibility.txt`，SHA-256 为
`4ec48d65d350a3a5c49142794050795d73fa0b80d444abc1b9581bb02428e67f`。

## 安全影响与未验证项

本变更不执行下载的 guest 二进制，不新增网络、设备、凭证、宿主路径、协议字段、migration、
crate 或服务边界；guest root block 仍只读，生产 profile 仍 fail closed。规范化步骤主动排除
ACL、SELinux 与 xattr，符合当前 Alpine 输入只依赖标准 mode/uid/gid 的开发契约；真实 KVM 回归
证明当前 guest 启动与 workload 未受破坏。

仍未验证：

- 不同发行版、`mke2fs/libext2fs`、GNU tar 或 Rust 工具版本之间的 bit-for-bit 可复现性；
- aarch64（当前 runtime 明确只支持 x86_64）和独立第二构建机复验；
- production jailer、uid/gid、cgroup、namespace、seccomp、watchdog 与恶意资源耗尽矩阵；
- 正式签名、来源证明、第三方许可证人工复核、分发权利与 release 性能。
