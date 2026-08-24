---
status: Validated
owners: OpenOJ operations, execution-plane and security maintainers
last_reviewed: 2026-08-25
applies_to: algorithm-c development runtime real-KVM compile, run, timeout and teardown smoke at commit 1fb82ec1e819082578a1ebf160a69b302ddb1b8f
references:
  - ../operations/algorithm-c-development-runtime.md
  - ../protocol/guest-vsock-v0alpha1.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../requirements/non-functional.md
  - ../security/threat-model.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - 2026-08-23-algorithm-c-runtime-smoke.md
---

# algorithm-c 真实 KVM smoke 验证（2026-08-25）

## 范围与结论

本记录验证 commit `1fb82ec1e819082578a1ebf160a69b302ddb1b8f` 及其父提交提供的
development-only `algorithm-c` runtime：固定镜像能在真实 KVM/Firecracker guest 内完成
vsock 协商、摘要校验的 C 源码上传、成功编译运行、编译失败、运行超时和幂等回收。

这为 `FR-ARTIFACT-001`、`FR-JUDGE-001/002/003`、`FR-RUNTIME-001` 与
`ACC-P0-001/005/008/009/010/016` 提供开发执行段的局部真实环境证据，并取代
`2026-08-23-algorithm-c-runtime-smoke.md` 中“本环境无 `/dev/kvm`”及其对应真实 guest
路径 `Unverified` 结论。它不构成完整 P0、production isolation、资源耗尽矩阵或性能验收。

## 环境与产物

- Arch Linux x86_64，kernel `7.1.8-zen1-3-zen`；AMD Ryzen 9 9950X3D，8 个可见 CPU，
  15 GiB 内存；VMware hypervisor 暴露 AMD-V。
- `/dev/kvm` 为 `crw-rw-rw- root:kvm`，KVM API version `12`；Firecracker `1.16.1`。
- Rust/Cargo `1.97.1`；guest agent 使用 release `x86_64-unknown-linux-musl` 静态构建，
  Rust 集成测试使用 debug 构建；没有预热、重复采样或性能结论。
- 完整工作区测试使用一次性 PostgreSQL `18.6` 容器，镜像摘要为
  `postgres@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2`；
  仅绑定 `127.0.0.1:55434`，数据目录位于 tmpfs，测试后容器已删除。
- 镜像由已校验的本地 cache 离线组装；root block 只读、无 TAP/network，`/work` 为
  `0700 root:root` 挂载点，启动后承载 64 MiB task-local tmpfs。

| 产物 | SHA-256 |
|---|---|
| kernel | `882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef` |
| rootfs | `15fed4ac44dd5f0de02a6c9f0d9c85e3207fd897a238ea9f08781f2a9eab904c` |
| guest agent | `e775d7d0fe71b8980a58ad02b2a9b8ac1195d6b32fc745db659750e46373cae2` |
| C toolchain source lock | `5d60f2080509113459fe14dca82594c87016f5a869c26351d9d6b2af55bb51c6` |
| manifest | `720941439956169beb8821f3afbf51842e82c8c5a71bc73fd61c2e7ded3dacc3` |
| SPDX SBOM | `aee7b475b81add5b58f497accdad5d4ef10a5ba8ad06e89007e2427e904ecddb` |

本地 `out/` 产物不进入 Git，也不构成发布制品。rootfs 跨构建 bit-for-bit 可复现性仍未验证，
因此每次运行继续以 manifest 中的实际摘要为准。

## RED → GREEN 与真实 workload

修复前，完整 manifest 校验通过，但严格 KVM smoke 在 10.31 秒后因 guest channel readiness
超时失败。一次性串口诊断确认 pinned kernel 已挂载 devtmpfs，PID 1 重复挂载 `/dev` 收到
`EBUSY` 后退出；使该步骤幂等后又暴露只读 rootfs 没有 `/work` 挂载点。

修复使 PID 1 通过 `/proc/mounts` 识别既有 devtmpfs，并在镜像供应阶段预创建
`0700 root:root /work`。随后严格测试在 1.74 秒内通过。单个 Rust 集成测试为每种情况启动
全新 microVM：

- 固定合法 C 源码编译运行，宿主校验输出 `42\n` 的 SHA-256 与 3 字节长度；
- 固定非法 C 源码产生非零 build exit 与有界诊断，不进入 run；
- 固定无限循环源码在 50 ms run budget 后返回 exit `124` 与有界诊断；
- 每个 case 重复 teardown 两次，并断言 VMM 终态以及 API/vsock socket 均已移除。

测试后无本次 Firecracker 进程或 task-local `openoj-algorithm-c-*` 工作目录残留；单独配置的
离线输入 cache 按预期保留。

## 命令与证据

以下命令通过：

```text
bash infra/runtime-images/algorithm-c/check.sh
OPENOJ_RUNTIME_OFFLINE=1 OPENOJ_RUNTIME_CACHE=/tmp/openoj-algorithm-c-cache bash infra/runtime-images/algorithm-c/provision.sh
(cd infra/runtime-images/algorithm-c/out && sha256sum --check --strict manifest.sha256)
debugfs -R 'stat /work' infra/runtime-images/algorithm-c/out/rootfs/rootfs.ext4
OPENOJ_REQUIRE_KVM=1 cargo test -p openoj-firecracker --test boot --locked -- --nocapture
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
# 在一次性 PostgreSQL 18.6 上设置 DATABASE_URL 和 OPENOJ_TEST_DATABASE_URL：
cargo test --workspace --all-targets --locked
# 用带 PyYAML 的隔离解释器运行五个适用 Skill 的 quick_validate.py
```

完整工作区测试共通过 145 个测试，包含 17 个 PostgreSQL control-spine 测试、5 个
control-plane/judge-node 跨进程测试、真实 KVM boot 测试和 production profile fail-closed
测试。`cargo deny` 只有仓库已有的重复版本告警；其余上述门禁及五个 Skill validator 均通过。

压缩原始摘要位于 `artifacts/2026-08-25-algorithm-c-runtime-kvm.txt`，SHA-256 为
`42ae0a323b686e118b5b21f3a994a9cce16fc262a4dbaff80b031e08381fae88`。

## 未验证与残余风险

- CLI/control-plane/judge-node 到真实 microVM 的完整判题闭环与对象存储正文；
- production jailer、独立 uid/gid、cgroup、namespace、seccomp 和宿主 watchdog；
- guest 网络主动探测、CPU/内存/进程/磁盘/I/O/无限输出、取消竞态和并发任务存活；
- rootfs bit-for-bit 可复现性、第三方许可证人工复核与正式分发权利；
- release 性能、密度、P50/P95/P99 和生产安全结论。

production profile 继续 fail closed；本验证没有放宽网络、凭证、设备或宿主路径边界。
