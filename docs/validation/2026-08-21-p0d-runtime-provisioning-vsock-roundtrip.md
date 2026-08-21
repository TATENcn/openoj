---
status: Validated
owners: OpenOJ maintainers
last_reviewed: 2026-08-21
applies_to: algorithm-c runtime image provisioning and real-KVM guest↔host vsock round-trip
references:
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../superpowers/plans/2026-08-20-p0d-firecracker-execution.md
  - ../protocol/guest-vsock-v0alpha1.md
  - ../requirements/acceptance.md
  - 2026-08-20-p0d-firecracker-boot.md
---

# algorithm-c runtime base 与 guest↔host vsock 往返验证（2026-08-21）

本记录验证 P0-D 执行平面的两件事：(1) `infra/runtime-images/algorithm-c/provision.sh`
能在无 root 的普通用户环境下供应一个不可变的 kernel + rootfs + 静态 guest agent
运行时基础镜像并记录摘要；(2) 该运行时在真实 KVM 上引导后，宿主经 Firecracker
vsock 桥与 in-guest agent 完成有界、版本化的 negotiate/upload/build 数据往返。

## 环境

- 同 `2026-08-18-kvm-firecracker-boot.md`：Arch Linux x86_64，内核 `7.1.8-zen1-3-zen`，
  KVM API v12，Firecracker 1.16.1 + jailer。普通用户 `cn059`（uid 1000，wheel），无 root。
- 运行时内容：
  - kernel：`hello-vmlinux.bin`（4.14.55-84.37.amzn2，含内建 virtio-vsock），
    sha256 `882fa465…c6014cef`。
  - rootfs：1.47.4 `mke2fs -d` + `fakeroot` 从 Alpine 3.20.0 musl minirootfs 组装
    （占位 64 MiB），只读镜像。
  - guest agent：`openoj-guest-agent` 静态 musl（static-pie，787 KiB），
    sha256 `acddead5…728a5b1a`，作为 PID-1 init 内 `exec` 运行。
- boot args：`console=ttyS0 reboot=k panic=1 pci=off init=/bin/openoj-init`；
  microVM 1 vCPU / 128 MiB，guest_cid 3，agent 端口 8266。
- `init=/bin/openoj-init` 挂载 proc/sys/devtmpfs，并把 `tmpfs size=64m` 挂到
  `/work`；根文件系统保持只读（不可变运行时），任务产物落 RAM tmpfs。

## 执行

供应镜像（免 root）：

```text
cd infra/runtime-images/algorithm-c
bash provision.sh
```

真实 KVM 往返集成测试（gated，无 KVM/未供应镜像时 skip）：

```text
OPENOJ_FC_TEST_IMAGES=$PWD/infra/runtime-images/algorithm-c/out \
  cargo test -p openoj-firecracker --test boot -- --nocapture
```

## 结果

- 供应成功：`out/kernel/vmlinux.bin`、`out/rootfs/rootfs.ext4`（64M）、
  `out/agent/openoj-guest-agent`、`out/manifest.json` + `manifest.sha256`。
  内核与 minirootfs 的 pin 摘要校验通过，任何不匹配会 fail closed。
- 集成测试两例通过：
  - `boots_a_real_microvm_and_reclaims_it ... ok`
  - `guest_agent_round_trip_over_vsock ... ok`
- vsock 往返数据（guest 内实际执行 build 阶段 `/bin/echo hello`）：
  negotiate → `{supported: true}`；upload_input → `{accepted: true}`；
  build → `{exit_code: 0, output_digest: 5891b5b5…6fbe03, output_bytes: 6,
  usage:{wall_time_ms:15,…, output_bytes:6}}`；heartbeat → `ack`。
- guest 串口确认 `openoj-guest-agent listening on vsock cid=any port=8266`；
  只读根下 `/work` 为 tmpfs，upload 可写（此前只读根导致的 `Read-only file
  system` 已由 tmpfs `/work` 消除）。

## 结论与映射

执行平面数据路径在真实 KVM 上贯通：宿主 `GuestChannel`（`CONNECT` + 定长前缀帧）
↔ Firecracker vsock 桥 ↔ in-guest `openoj-guest-agent` 有界、版本化消息交换正常，
guest 执行受限命令并回传阶段证据与资源摘要，宿主以证据构成观察而非信任 guest 自报。
这落实 `ACC-P0-001` 的执行段、`ACC-P0-008/009/010` 的运行时对象与阶段证据基础，
并跟进 `ADR-0005` 与 `2026-08-20-p0d-firecracker-execution` 计划 Task 6。

## 未验证

- algorithm-c 语言工具链（gcc/musl-dev/binutils）尚未供入运行时基础镜像：
  免 root 的 apk 安装需要 fakechroot/受控构建宿主，属后续切片。运行时
  `manifest.json` 标记 `toolchain.pending` 且 `verified:false`，本记录**不**声明
  生产语言矩阵或可编译任意用户 C 代码。
- 生产隔离层（jailer/cgroup/seccomp/uid-gid）、恶意 workload/资源耗尽/取消竞态、
  多任务并发与宿主存活、性能基线均未验证；这些仍由 `ACC-P0-002/003/007/014/015/
  016` 覆盖并留待 P0 后续切片。
- rootfs.ext4 字节级不可复现（mke2fs 每次生成随机 UUID/时间戳）；manifest 记录
  本次构建摘要并在配置端强制，不等同于确定性重建。
