---
status: Validated
owners: OpenOJ control-plane, execution-plane, operations and security maintainers
last_reviewed: 2026-08-25
applies_to: development-only algorithm-c CLI/PostgreSQL/UDS/judge-node/Firecracker result loop at commit c492288bcfc25485ca1bead9e9ff4ad31636ff4d
references:
  - ../architecture/overview.md
  - ../architecture/workspace-and-crates.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../operations/algorithm-c-development-runtime.md
  - ../requirements/acceptance.md
  - ../requirements/functional.md
  - ../security/threat-model.md
  - ../security/trust-boundaries.md
  - 2026-08-25-algorithm-c-runtime-kvm.md
---

# algorithm-c 真实 microVM 进程闭环验证（2026-08-25）

> 后续证据：`2026-08-25-algorithm-c-process-failures.md` 已在真实 CLI/PostgreSQL/UDS/
> judge-node/Firecracker 链路验证 `CompileError` 与 `TimeLimitExceeded` 的持久化、Evidence、
> 后续 stage 跳过和资源回收。本记录保留原始 Accepted 闭环证据。

## 范围与结论

本记录验证 commit `c492288bcfc25485ca1bead9e9ff4ad31636ff4d` 的 development-only
闭环：CLI 创建 Evaluation，PostgreSQL 保存任务，control-plane 通过私有 UDS 发出租约，独立
judge-node 在 Firecracker guest 内编译运行固定 C Artifact，宿主校验输出摘要，并把结构化
Firecracker 结果持久化。

这关闭 `2026-08-25-algorithm-c-runtime-kvm.md` 中“CLI/control-plane/judge-node 到真实
microVM”针对固定 development Artifact 的 `Unverified` 项，为 `FR-ARTIFACT-001`、
`FR-EVAL-001`、`FR-JUDGE-001/002`、`FR-RUNTIME-001`、`FR-RESULT-001` 与
`ACC-P0-001/005/008/009/010/016` 提供局部目标环境证据。它不验收正式对象存储、任意用户
Artifact、HTTP API、production isolation 或完整 P0。

## 数据流与边界

```text
fixed main.c -> CLI -> PostgreSQL -> control-plane UDS lease -> judge-node
     -> digest/size/media/runtime match -> Firecracker/vsock -> guest cc + run
     -> host output-digest check -> fenced result submit -> PostgreSQL
```

- `OPENOJ_FC_SOURCE` 是受信部署配置中的绝对普通文件；请求不能提供宿主路径或 argv。
- 源码最多 256 KiB；启动 guest 前匹配 SHA-256、精确字节数、`text/x-csrc` 和 Runtime 摘要。
- build/run 使用固定 argv；guest 无 TAP/network 和平台凭证，root block 只读。
- guest exit 0 不足以产生 `Accepted`；宿主还必须匹配固定预期输出摘要。
- 结果保留 Firecracker executor、`production_eligible=false`、node ID、Runtime 与 run Evidence。
- production profile 继续无条件 fail closed；正式 Artifact backend 可以移除本地 bridge，不改变
  canonical request 或 guest v0alpha1 消息。

## 环境、输入与产物

- Arch Linux，kernel `7.1.8-zen1-3-zen`；AMD Ryzen 9 9950X3D，8 个可见 CPU，VMware
  暴露 AMD-V；15 GiB 内存、4 GiB swap。
- `/dev/kvm` 为 `crw-rw-rw- root:kvm`，KVM API `12`；Firecracker `1.16.1`。
- 一次性 PostgreSQL `18.6` 只绑定 `127.0.0.1:55435`，数据位于 tmpfs，测试后容器删除；
  image digest 为
  `postgres@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2`。
- 固定源码 SHA-256 为
  `c2329bffd207edd619d4ca7c7cdd872b76374e1a50421f705020671ca1f85556`；输出 `42\n`
  SHA-256 为 `084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0`。

| 产物 | SHA-256 |
|---|---|
| kernel | `882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef` |
| rootfs | `7a7f28d788d587cf8bb58b3762c714d31472e4057bbb93e3dc0bdcfe5e3d32cd` |
| guest agent | `e775d7d0fe71b8980a58ad02b2a9b8ac1195d6b32fc745db659750e46373cae2` |
| runtime manifest | `c59c795d0205af522050f03608769703ab6d30a7b2db2bbc46b834373a6562a7` |
| SPDX SBOM | `aee7b475b81add5b58f497accdad5d4ef10a5ba8ad06e89007e2427e904ecddb` |

本次 rootfs 摘要与上一记录不同，而 guest agent 摘要相同；跨重建 bit-for-bit 可复现性仍为
`Unverified`，因此结论只适用于上表精确摘要。

## RED → GREEN

首次严格进程 E2E 在 25.68 秒后没有终态。持久化诊断显示 prepare 失败：judge-node 的
Firecracker 组装缺少 `root=/dev/vda ro init=/sbin/openoj-init`，且使用 128 MiB，而严格
runtime smoke 已验证的镜像契约是 256 MiB。对齐 cmdline 与 machine shape 后，相同测试在
8.21 秒通过。

GREEN 断言覆盖：真实 CLI/control-plane/judge-node/Firecracker 进程、PostgreSQL 租约与
结果、guest C 编译运行、`Accepted`、Firecracker 非生产 provenance、node ID、run Evidence
摘要及 API/vsock 路径回收。

## 门禁

以下均通过：

```text
OPENOJ_RUNTIME_OFFLINE=1 OPENOJ_RUNTIME_CACHE=/tmp/openoj-algorithm-c-cache bash infra/runtime-images/algorithm-c/provision.sh
bash infra/runtime-images/algorithm-c/check.sh
(cd infra/runtime-images/algorithm-c/out && sha256sum --check --strict manifest.sha256)
OPENOJ_REQUIRE_KVM=1 OPENOJ_TEST_DATABASE_URL=<PostgreSQL 18.6> cargo test -p openoj-control-plane --test process_e2e postgres_uds_and_judge_node_reach_a_persisted_real_microvm_result --locked -- --nocapture
OPENOJ_REQUIRE_KVM=1 DATABASE_URL=<PostgreSQL 18.6> OPENOJ_TEST_DATABASE_URL=<same> cargo test --workspace --all-targets --locked
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny --locked check advisories bans sources
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
```

完整 workspace 共 149 个测试通过、0 失败；包括严格三用例 KVM runtime smoke、真实进程
闭环、17 个 PostgreSQL control-spine 测试、摘要/大小拒绝、错误输出 Wrong Answer、回收和
production fail-closed。依赖检查只有仓库已有重复版本告警。四个适用 Skill validator 通过。

压缩原始摘要位于 `artifacts/2026-08-25-algorithm-c-process-e2e.txt`，SHA-256 为
`270553e45caa5cf35377ae3753e32caa5d6d45dd403b36b7b288e5cdd0d9713e`。

## 未验证与残余风险

- 正式对象存储 Artifact 正文、短期授权和任意用户上传；
- HTTP/API、身份、审计关联和外部 request ID 查询；
- production jailer、独立 uid/gid、cgroup、namespace、seccomp 与宿主 watchdog；
- CPU/内存/process/disk/I/O/无限输出、并发 guest、取消竞态和宿主存活矩阵；
- rootfs bit-for-bit 可复现性、第三方许可证人工复核、release 性能与密度。

本验证没有放宽网络、秘密、宿主路径、固定 argv、消息上限或 production fail-closed 边界。
