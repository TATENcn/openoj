---
status: Validated
owners: OpenOJ maintainers
last_reviewed: 2026-08-20
applies_to: P0-D Firecracker microVM boot via the openoj-firecracker adapter
references:
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../superpowers/plans/2026-08-20-p0d-firecracker-execution.md
  - 2026-08-18-kvm-firecracker-boot.md
---

# 本机 Firecracker microVM 启动验证（通过 openoj-firecracker 适配器，2026-08-20）

## 环境

- 同 `2026-08-18-kvm-firecracker-boot.md`：Arch Linux x86_64、KVM API v12
  （`/dev/kvm`）、Firecracker 1.16.1 + jailer。
- image：官方 `hello-vmlinux.bin` + `hello-rootfs.ext4`（仅启动 plumbing，非生产
  workload）。

## 执行

运行 `openoj-firecracker` 的 KVM 门控集成测试：

```text
OPENOJ_FC_TEST_IMAGES=$PWD/infra/runtime-images/algorithm-c/out \
  cargo test -p openoj-firecracker --test boot -- --nocapture
```

`FirecrackerVm::bootstrap` 依次 `PUT /boot-source`、`/drives/rootfs`、
`/machine-config`、`/vsock`、`/actions InstanceStart`，全部返回 2xx，microVM 进入
`Started`，随后 `terminate()` 幂等回收（kill 子进程 + 移除 socket）。

## 结果

- 测试通过：`boots_a_real_microvm_and_reclaims_it ... ok`。
- 上述 API 请求全部 `204 No Content`（machine-config 使用 `smt` 字段、vsock 使用
  `vsock_id/guest_cid/uds_path` 字段，符合 Firecracker 1.16 schema）。

## 结论

`openoj-firecracker` 能真实启动并回收一个 microVM，宿主侧 control-API/vsock 设备
配置与生命周期状态机工作。这验证了执行平面所需的进程与设备面基线。

## 未验证

- 生产隔离（jailer/cgroup/seccomp）、guest↔host vsock 数据往返、
  guest agent 在 rootfs 内运行、工具链编译与宿主侧真实 check 均未验证。
  这些需要构建并启动一个含 guest agent + 单一语言工具链的算法 runtime image，
  且宿主对照隐藏测试数据依赖对象存储正文（P0 非目标）。
