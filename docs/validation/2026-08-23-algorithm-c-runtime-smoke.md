---
status: Implemented
owners: OpenOJ operations, execution-plane, protocol and security maintainers
last_reviewed: 2026-08-23
applies_to: development-only algorithm-c runtime image and strict real-KVM smoke contract
references:
  - ../operations/algorithm-c-development-runtime.md
  - ../protocol/guest-vsock-v0alpha1.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../requirements/non-functional.md
  - ../security/threat-model.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - 2026-08-20-p0d-firecracker-boot.md
  - 2026-08-25-algorithm-c-runtime-kvm.md
---

# algorithm-c 开发运行时与严格 KVM smoke（2026-08-23）

> 后续证据：`2026-08-25-algorithm-c-runtime-kvm.md` 已在 commit `1fb82ec` 和真实 KVM
> 环境关闭本记录中的 guest readiness、C 编译运行、编译错误、运行超时与 socket 回收
> `Unverified` 项。本记录保留当时无 KVM 环境的原始结论，不用于描述当前验证状态。

## 范围与结论

本记录针对 commit `623913e009156cce50b4422e51f30b0ab68fcdf6` 及其前置五个提交，验证
`FR-ARTIFACT-001`、`FR-JUDGE-001/002/003`、`FR-RUNTIME-001` 与
`ACC-P0-001/005/008/009/010/016` 的开发态局部切片。

已实现并在无 KVM 环境验证：固定来源与摘要的 kernel/minirootfs/C 工具链、静态 guest agent、
只读 rootfs 与有界 `/work` init、manifest/SPDX、上传摘要拒绝、bounded vsock readiness、API/vsock
socket 回收，以及严格测试的环境 fail-closed。真实 microVM 内的编译、运行、失败与超时断言因本次
环境没有 `/dev/kvm` 而保持 `Unverified`，因此状态是 `Implemented`，不是 `Validated`。

## 环境与构建

- Arch Linux x86_64，kernel `7.1.8-zen1-3-zen`；
- AMD Ryzen 9 9950X3D，8 个可见 CPU，AMD-V；15 GiB 内存；
- Rust/Cargo `1.97.1`；Firecracker `1.16.1`；
- `/dev/kvm` 不存在；PostgreSQL 未运行，`DATABASE_URL` 未设置；
- guest agent 使用 `--release --target x86_64-unknown-linux-musl`；Rust 门禁使用 debug/test 构建；
- rootfs 使用已校验的本地 cache 离线组装一次；没有性能预热、重复采样或性能结论；
- 构建过程只把下载的 Alpine 文件作为数据校验和解包，没有在宿主执行 guest 第三方二进制。

本地忽略目录 `infra/runtime-images/algorithm-c/out/` 的产物摘要：

| 产物 | SHA-256 |
|---|---|
| kernel | `882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef` |
| rootfs | `0336f040e0dfaeffd37c50f44156f5c26f45ef080908e0a2feeefa8d207a053e` |
| guest agent | `e775d7d0fe71b8980a58ad02b2a9b8ac1195d6b32fc745db659750e46373cae2` |
| C toolchain source lock | `5d60f2080509113459fe14dca82594c87016f5a869c26351d9d6b2af55bb51c6` |
| manifest | `b5b164a8ea23898e5c96021e1257e69ef576a350d286132c2da8e7759af0522b` |
| SPDX SBOM | `aee7b475b81add5b58f497accdad5d4ef10a5ba8ad06e89007e2427e904ecddb` |

这些本地产物不进入 Git，也不构成发布制品。rootfs 的跨构建 bit-for-bit 可复现性尚未验证；每次
执行仍必须使用 manifest 记录的实际内容摘要。

## 固定 workload 与严格测试

严格测试为每个 case 启动全新 microVM，固定 1 vCPU/256 MiB、只读 rootfs、无网络设备、
guest CID `3`、port `8266`，先验证 manifest 和 `manifest.sha256`，再进行 v0alpha1 capability
negotiation 与摘要校验上传：

- 成功 fixture SHA-256
  `c2329bffd207edd619d4ca7c7cdd872b76374e1a50421f705020671ca1f85556`，固定
  `/usr/bin/cc -std=c17 -O2 -pipe /work/inputs/main.c -o /work/solution`，预期输出 `42\n`
  的摘要为 `084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0`；
- 编译失败 fixture SHA-256
  `45c508a05870369dd65fb951ee804127925b454fa0078ac6e37f84f5a1e3fa06`，必须产生非零 build
  exit 和有界诊断，且不进入 run；
- 无限循环 fixture SHA-256
  `84950edbf9514ebef845181f9f296bf2a71be3032f0d1d27b28b93fe1fd57f54`，build 成功后 50 ms
  run budget 必须返回确定的 timeout exit `124` 和有界诊断；
- 每个 case 都要求两次 teardown 成功、phase 为 `Terminated`、API/vsock socket 不残留。

## 门禁结果

- `OPENOJ_RUNTIME_OFFLINE=1 OPENOJ_RUNTIME_CACHE=/tmp/openoj-algorithm-c-cache bash infra/runtime-images/algorithm-c/provision.sh`：通过；
- 在 `out/` 运行 `sha256sum --check manifest.sha256`：5 个产物全部通过；`debugfs` 确认 init、
  guest agent 与 `/usr/bin/cc` 为 uid/gid `0:0`；
- `cargo fmt --all -- --check`：通过；
- `cargo check --workspace --all-targets --locked`：通过；
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过；
- `cargo test --workspace --all-targets --locked --exclude openoj-storage --exclude openoj-cli --exclude openoj-control-plane`：
  113 个测试函数通过；其中 algorithm-c 集成测试先验证镜像清单，再因 `/dev/kvm` 缺失明确跳过
  microVM 部分；
- `OPENOJ_REQUIRE_KVM=1 cargo test -p openoj-firecracker --test boot --locked -- --nocapture`：
  按预期失败，原因为 `/dev/kvm is absent or not read-write accessible`，证明严格模式未把 skip
  伪装成通过；
- `python3 scripts/check-workspace.py`、`cargo deny --locked check advisories bans sources`、
  `bash scripts/check-docs.sh`、`bash infra/runtime-images/algorithm-c/check.sh`：通过；cargo-deny
  仅报告仓库既有 duplicate dependency warning；
- 完整 `cargo test --workspace --all-targets --locked` 在既有 PostgreSQL 测试
  `migrate_submit_replay_and_status_use_the_durable_store` 因 `DATABASE_URL` 未设置而停止，不能解释为
  数据库套件通过或本变更回归。

压缩命令记录位于 `artifacts/2026-08-23-algorithm-c-runtime-smoke.txt`，SHA-256 为
`b09e107c7a1c66a08af65485423951f28490b937f0de501353886a40ea4b7df3`。本地 runtime image 与完整
终端日志未提交。

## 协议兼容与未验证项

`upload_input.digest` 的精确编码和 mismatch 拒绝属于既有 v0alpha1 内容摘要语义的行为兼容落实，
没有 wire 字段或版本变化。唯一声明的组合仍是 host v0alpha1 ↔ guest v0alpha1；其他版本组合、
rolling upgrade/downgrade 与独立 conformance 实现未验证。

剩余未验证项：

- 真实 KVM 上的 guest readiness、C 编译/运行、编译错误、运行超时和 VMM/socket 回收；
- rootfs 跨构建 bit-for-bit 可复现性和第三方许可证人工复核；
- PostgreSQL、CLI/API 到对象存储正文和数据库结果的完整判题闭环；
- production jailer、独立 uid/gid、cgroup、namespace、seccomp、宿主 watchdog；
- guest 网络探测、恶意 workload、CPU/内存/进程/磁盘/I/O/无限输出、取消竞态、并发宿主存活；
- release 性能、密度、P50/P95/P99 或生产安全结论。
