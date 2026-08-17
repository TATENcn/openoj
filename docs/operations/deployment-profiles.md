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
- P0-C 使用 `openoj-control-plane` 和 `openoj-judge-node` 两个独立进程；前者只监听绝对路径、`0700` 父目录下的 `0600` Unix Domain Socket，后者只连接该 UDS，不读取 `OPENOJ_DATABASE_URL`。
- control-plane 启动需要 `OPENOJ_DATABASE_URL`、`OPENOJ_JUDGE_CONTROL_SOCKET` 和默认拒绝的 `OPENOJ_JUDGE_NODES` allowlist（`node_id:capability[,capability]`，多节点以分号分隔）；judge-node 需要 `OPENOJ_JUDGE_CONTROL_SOCKET`、`OPENOJ_JUDGE_NODE_ID` 和显式 `OPENOJ_JUDGE_EXECUTOR=development_mock`。
- socket 已存在、相对路径、非私有父目录、未知 node/capability、协商版本不兼容或随机源不可用时必须拒绝启动/领取；不得以 TCP、宿主执行或隐式 mock 回退。
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
