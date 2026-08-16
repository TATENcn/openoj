---
status: Proposed
owners: OpenOJ operations maintainers
last_reviewed: 2026-08-16
applies_to: deployment profiles
references:
  - ../architecture/overview.md
  - ../security/trust-boundaries.md
  - ../architecture/decisions/0001-rust-and-firecracker.md
---

# 部署 Profile

## Development

- 控制平面、数据库和 mock executor 可以单机运行。
- 无 KVM 时只能验证领域、协议、存储和 UI，不得给出真实隔离或 microVM 性能结论。
- 开发凭证和数据不得复用生产环境。
- UI/API 必须显式标识 mock executor 结果。

## Single-node production

- 控制平面和 judge node 使用不同进程、身份和最小凭证；推荐不同主机。
- judge node 运行受支持 Linux/KVM、固定 Firecracker、kernel、rootfs 和 guest agent。
- 使用 jailer 或等价约束、独立 uid/gid、cgroup、namespace、seccomp、默认无 guest 网络和宿主 watchdog。
- PostgreSQL 和对象存储具备备份、恢复和访问审计。
- 节点支持停止接单、排空、强制回收和版本不兼容拒绝。

## Distributed

P2 前不构成承诺。未来需要定义节点身份、租约、容量、故障域、配额、滚动升级、版本矩阵、跨区域数据和灾难恢复。不得仅因部署在 Kubernetes 就声明具备 Firecracker 安全或高可用。

## 支持矩阵

具体 Linux、内核、CPU 架构、KVM 和 Firecracker 版本必须在原型实测后进入验证矩阵。没有记录的环境一律为未验证。
