---
status: Accepted
owners: OpenOJ protocol maintainers
last_reviewed: 2026-08-20
applies_to: guest↔host vsock message protocol (P0 execution plane)
references:
  - ../architecture/decisions/0005-firecracker-execution-plane.md
  - ../requirements/acceptance.md
  - versioning.md
---

# Guest↔Host vsock 消息协议（v0alpha1）

本协议定义 judge node（`openoj-firecracker`）与 guest 内命令 agent
（`openoj-guest-agent`）之间经 Firecracker vsock 交换的消息。规范实现与更新必须通过
`skills/evolve-openoj-protocol`。

## 载体与边界

- 每条消息是一个**定长前缀帧**：4 字节大端长度 + JSON body；总帧最大
  `1_048_576` 字节，长度前缀超过该值被拒绝（`BoundedLengthExceeded`）。
- 每个 JSON body 必须含字符串 `type`（≤32 字节）与 `version = "v0alpha1"`；
  缺失、未知或过长 type 以及版本不匹配被拒绝（fail closed）。
- `openoj-guest-protocol` crate 是唯一 wire 事实源；禁止手写第二份漂移类型。
- 字段级最大：capability 数 `32`、每个 capability `32` 字节、digest `64` 字节、
  inline input/evidence `262_144` 字节、参数数 `32`、单参数 `256` 字节、诊断数 `32`、
  单诊断 `4096` 字节（超限截断并标记 `truncated`）。

## 消息表

| type | 方向 | 含义 |
|---|---|---|
| `negotiate` | host→guest | 提供帧版本与排序去重 capability 集 |
| `negotiated` | guest→host | `supported: bool` 接受/拒绝版本与能力 |
| `upload_input` | host→guest | 写入一个有名字、摘要、有界 payload 的输入 |
| `upload_ack` | guest→host | 确认输入已接收 |
| `build` | host→guest | 运行编译阶段（有界 argv + 墙钟上限） |
| `run` | host→guest | 运行执行阶段（有界 argv + 墙钟上限） |
| `stage_output` | guest→host | 阶段退出码、输出摘要/字节、usage、有界诊断 |
| `stage_evidence` | host→guest | 请求一个结构化证据 artifact |
| `evidence_ack` | guest→host | 确认证据请求 |
| `cancel` | host→guest | 取消当前阶段并收敛终态 |
| `cancelled` | guest→host | 确认取消 |
| `heartbeat` / `ack` | host↔guest | 存活探测 |

## 安全与语义约定

- guest 输出一律由宿主重新按不可信输入处理；最终 Verdict/Score 由宿主侧
  evaluator 依据证据形成，guest 自报成功不构成终态（`ACC-P0-010`）。
- `upload_input.digest` 是 payload 的 64 字符小写十六进制 SHA-256（不含
  `sha256:` 前缀）。格式错误或摘要不匹配时 guest 返回 `accepted: false`，且不得落盘。
  `stage_output.output_digest` 使用同一编码，覆盖被计入 `output_bytes` 的 stdout+stderr。
- 命令使用**已验证的 argv 数组**（执行器在宿主侧构造并绑定），不提供通用
  shell、宿主路径访问或任意网络（`ACC-P0-005`）。
- 敏感内容（token、宿主路径、canonical payload）不进入日志或指标标签。
- 消息有长度上限、状态机与超时；畸形/超限输入触发确定性拒绝。

## 版本与兼容

当前只声明 `host v0alpha1 <-> guest v0alpha1`。不兼容变更须新增版本并在
`versioning.md` 记录；未知版本 fail closed，不宣称滚动兼容。
