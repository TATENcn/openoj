---
status: Validated
owners: OpenOJ maintainers
last_reviewed: 2026-08-18
applies_to: local KVM/Firecracker availability evidence (execution plane enabler)
references:
  - ../architecture/decisions/0001-rust-and-firecracker.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../superpowers/specs/2026-08-17-p0d-execution-plane-design.md
---

# 本机 KVM/Firecracker 启动验证（2026-08-18）

## 环境

- Arch Linux，x86_64；内核 `7.1.8-zen1-3-zen`；AMD Ryzen 9 9950X3D 16-Core（VMware full virtualization，已暴露嵌套虚拟化）。
- KVM：`/dev/kvm` 存在且当前用户可打开（`crw-rw-rw-`），KVM API version 12，`KVM_CAP_NR_VCPUS`=8，`kvm_amd` + `kvm` 模块已加载，CPU 有 `svm` 标志。
- Firecracker 1.16.1 + jailer 1.16.1（pacman 安装）。

## 执行

使用 Firecracker 官方 legacy hello 测试镜像（不用于生产 workload）：

```text
kernel : spec.ccfc.min/img/hello/kernel/hello-vmlinux.bin  (ELF x86-64, 21 MiB)
rootfs : spec.ccfc.min/img/hello/fsfiles/hello-rootfs.ext4 (ext4, 30 MiB)
```

启动序列：`firecracker --api-sock <tmp>` → `PUT /boot-source`（boot_args `console=ttyS0 reboot=k panic=1 pci=off`）→ `PUT /drives/rootfs`（is_root_device）→ `PUT /machine-config`（1 vCPU / 128 MiB）→ `PUT /actions InstanceStart`。

## 结果

- 所有 API 请求 `204 No Content`；`fc_vcpu 0` 恢复运行。
- guest 在 microVM 内真实引导：串口输出 Alpine Linux 3.8（kernel `4.14.55-84.37.amzn2.x86_64`）启动日志直至 `localhost login:` 提示符。
- 之后进程被正常终止，临时文件清理，无残留 VMM。

## 结论

本机具备运行 Firecracker microVM 的目标环境能力（KVM 可用 + Firecracker 已安装 + 可引导 guest）。这取代了 `2026-08-16-p0-evaluation-kernel.md` 中"当前环境无 /dev/kvm"的过时陈述，并作为执行平面实现的目标环境证据起点。

## 未验证

- 本记录只证明 microVM 可启动，不构成生产隔离、性能、恶意 workload 存活或资源回收结论。
- jailer 配置、cgroup/seccomp、vsock 协议、build/run/check 与 runtime image 供应链均未实现。
