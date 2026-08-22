---
status: Implemented
owners: OpenOJ operations and security maintainers
last_reviewed: 2026-08-23
applies_to: development-only algorithm-c runtime image and KVM smoke validation
references:
  - deployment-profiles.md
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../requirements/functional.md
  - ../requirements/non-functional.md
  - ../requirements/acceptance.md
  - ../security/threat-model.md
  - ../security/trust-boundaries.md
  - ../development/testing.md
  - ../governance/licensing.md
---

# algorithm-c 开发运行时契约

## 目标与边界

`algorithm-c` 是 P0 执行平面的单语言开发运行时。它只用于证明固定源码 fixture 能在
Firecracker guest 内经受限 vsock 协议完成上传、编译、运行、宿主校验和回收。本契约落实
`FR-JUDGE-001/002`、`FR-RUNTIME-001` 与 `ACC-P0-001/005/008/009/010/016` 的局部切片，
不构成完整 P0 验收或生产隔离声明。

本运行时不得用于公开服务或生产 workload。`OPENOJ_FC_PRODUCTION=1` 必须继续 fail closed；
缺少独立 uid/gid、jailer、cgroup、namespace、seccomp 与宿主 watchdog 时不得改变该边界。

## 不可变输入与输出

供应脚本必须从仓库内版本化 lock 文件读取每个外部输入的固定版本、HTTPS 来源、SHA-256
与许可证状态。不得用环境变量替换来源或摘要；本地缓存只按摘要命中，校验失败必须在解包前
拒绝。第三方权利状态保持 `pending-review`，本仓库不提交或分发生成的二进制镜像。

`infra/runtime-images/algorithm-c/out/` 是忽略的本地产物目录，至少包含：

```text
kernel/vmlinux.bin
rootfs/rootfs.ext4
agent/openoj-guest-agent
manifest.json
manifest.sha256
sbom.spdx.json
```

`manifest.json` 必须记录 runtime/version/architecture、构建时间策略、kernel、rootfs、guest
agent、C 工具链的来源与 SHA-256、rootfs 只读属性、guest 网络策略和验证状态。manifest 与
SBOM 自身也必须进入 `manifest.sha256`。摘要或架构不匹配时测试和执行配置必须拒绝。

## Guest 文件系统与启动

- root block device 以只读方式挂载；不得把宿主任意路径映射进 guest。
- PID 1 只挂载最小 `proc`、`sysfs`、`devtmpfs` 与有大小上限的 `/work` tmpfs，然后以前台
  方式启动 `openoj-guest-agent`。agent 退出时 guest 停止，不提供登录 shell 或守护服务。
- guest 不配置 TAP、网络接口、DNS、平台凭证或对象存储密钥；唯一任务数据通道是有界
  `v0alpha1` vsock 消息。
- C 编译和运行命令由宿主从固定 executable ID 构造 argv 数组，不接受提交提供的 shell
  文本。开发 smoke 固定使用 `/usr/bin/cc`、`/work/inputs/main.c` 与 `/work/solution`。
- 上传、stdout/stderr、诊断、墙钟和 `/work` 容量必须保持有界；guest 输出只能作为宿主
  checker 的不可信证据，不能自行声明最终 Verdict。

## 验证契约

普通无 KVM CI 可以显式报告 `Skipped`，但不得把它记录为真实 guest 验证。设置
`OPENOJ_REQUIRE_KVM=1` 后，缺少 `/dev/kvm`、Firecracker、完整镜像、manifest 摘要或 guest
agent readiness 必须让测试失败。

真实目标环境至少验证：

1. microVM 启动且 guest agent 完成 capability negotiation；
2. 上传固定 `main.c`，在 guest 内编译并运行，宿主校验预期输出摘要；
3. 编译错误和运行超时返回确定阶段结果，不伪造后续成功；
4. 重复 teardown 幂等，VMM 与 socket 不残留；
5. guest 未配置网络设备，未接收平台秘密。

验证记录必须包含主机、内核、KVM、Firecracker、构建类型、所有镜像摘要、输入 fixture、
完整命令、结果和未验证项。没有真实 KVM 证据时文档状态保持 `Proposed` 或
`Implemented`，不得提升为 `Validated`。

## 非目标与回退

本切片不实现对象存储正文、CLI/API 到数据库的完整判题闭环、隐藏测试、多语言、production
jailer/cgroup/seccomp/watchdog、恶意 workload 矩阵或性能结论。回退时删除本地 `out/` 并
恢复到 hello-image boot 测试；不得回退到宿主直接编译或执行提交。
