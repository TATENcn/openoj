---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-20
applies_to: P0-D execution-plane implementation
references:
  - ../specs/2026-08-17-p0d-execution-plane-design.md
  - ../../architecture/decisions/0005-firecracker-execution-plane.md
  - ../../requirements/functional.md
  - ../../requirements/non-functional.md
  - ../../requirements/acceptance.md
---

# P0-D Firecracker 执行平面实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 P0-C 的 mock 闭环推进到真实执行段：judge node 通过 jailer 启动 Firecracker microVM，guest agent 经 vsock 执行受限命令，宿主侧 evaluator 完成 check/aggregate，返回结构化结果。production 配置必须 fail closed，development mock 保持显式且不可被生产选择。

**Architecture:** `openoj-firecracker` 是唯一持有平台/jailer/VMM/vsock 能力的系统适配器；`openoj-guest-protocol` 定义受限、有界、版本化的 guest↔host vsock 消息；`openoj-guest-agent` 是 guest 内最小二进制；`openoj-evaluator` 是 transport/VMM-neutral 阶段执行器，只依赖 canonical Evaluation 语义；`openoj-judge-node` 组合 Firecracker executor，仅当显式配置真实 profile 且强制层完整时启用。

**Tech Stack:** Rust 1.97.1、Tokio、Firecracker 1.16.1 + jailer、vsock（`/dev/vhost-vsock`）、现有 canonical JSON Schema 协议。运行时镜像：不可变 kernel/rootfs/guest-agent，内容寻址 + SBOM。

## Global Constraints

- Scope 是 `FR-JUDGE-001/002/003`、`FR-RUNTIME-001`、`FR-RESULT-001` 相关验收与 `ADR-0005`。
- 不可信代码只在 jailer/Firecracker 隔离边界内编译运行；production 路径永不回退宿主执行，development mock 永不进入 production Profile。
- guest 默认无网络；本切片不开放 TAP/NAT/DNS，不引入对象存储正文上传或短期凭证。
- 每个阶段保存状态、起止/资源摘要、有界诊断与 Evidence 引用；阶段失败不伪造后续成功；guest 不能自证终态。
- 回收幂等：重复 teardown、VMM 无响应/watchdog 强杀、宿主重启 reconciler 清理。
- 所有新依赖精确固定、feature-minimized、文档化、lockfile 复核并 cargo-deny 检查。
- 手工构建的 runtime image 必须记录来源、摘要与 SBOM；摘要/架构不匹配时执行被拒绝。

## File Map

```text
crates/openoj-guest-protocol/                          # bounded versioned vsock message codec
crates/openoj-firecracker/                             # jailer/VMM/vsock adapter + lifecycle
crates/openoj-guest-agent/                             # guest 内受限 vsock 命令 agent
crates/openoj-evaluator/                               # prepare/build/run/check/aggregate stage runner
apps/openoj-judge-node/                                # real Firecracker executor wiring (fail-closed)
infra/runtime-images/algorithm-c/                      # kernel/rootfs/guest-agent/SBOM provisioning
docs/protocol/guest-vsock-v0alpha1.md                  # canonical guest↔host message table
docs/architecture/, docs/security/, docs/operations/, docs/validation/  # synchronized facts/evidence
```

---

### Task 1: 接受 ADR-0005 并建立实现计划

- [ ] 标记 ADR-0005 为 Accepted（维护者已批准进入实现），新增本计划文档，同步 RISC/威胁/架构事实的受影响条目。
- [ ] 运行 `bash scripts/check-docs.sh`。

### Task 2: 建立有界 guest↔host vsock 协议 crate

- [ ] 写测试：消息往返、长度上限、未知/畸形消息拒绝、版本协商、敏感字段不发日志。
- [ ] 观察新 crate 缺失 → 测试失败。
- [ ] 实现 `openoj-guest-protocol`：消息表（negotiate/upload_input/build/run/stage_evidence/cancel/heartbeat）、定长前缀 frame codec、错误类别、转换；写入 canonical 文档 `docs/protocol/guest-vsock-v0alpha1.md`。
- [ ] 添加精确固定依赖，更新依赖方向/技术栈/workspace 事实，测试转绿。

### Task 3: 实现 jailer/VMM 系统适配器 `openoj-firecracker`

- [ ] 写状态机/配置/错误测试：生命周期转换、资源上限校验、unsafe 收敛、幂等 teardown。
- [ ] 观察缺失 crate → 失败。
- [ ] 实现 JailerConfig/FirecrackerConfig/CgroupLimits、生命周期状态机（provisioning→booted→…→terminated）、jailer+firecracker 启动、HTTP-over-UDS 到 Firecracker API、vsock 收发、幂等回收；平台 `unsafe` 收敛到最小系统模块。
- [ ] 新增真实 KVM 集成测试（无 KVM 时 skip）：启动 microVM、等待 guest agent、交换 negotiate 消息、teardown。
- [ ] 更新 workspace/architecture 事实，测试转绿。

### Task 4: 实现 guest 内受限命令 agent `openoj-guest-agent`

- [ ] 写消息处理测试：受支持命令、畸形/超限拒绝、取消、敏感路径拒绝。
- [ ] 实现 guest 侧 vsock 服务，执行受限命令（upload/build/run/evidence/heartbeat/cancel），不提供通用 shell，输出转交宿主。
- [ ] 静态/自包含构建以纳入 rootfs；记录构建来源与摘要。

### Task 5: 实现宿主侧阶段执行器 `openoj-evaluator`

- [ ] 写阶段测试：逐阶段状态/资源/证据、阶段失败不伪造后续、check 产出 Decision、guest 输出重视为不可信。
- [ ] 实现 transport-neutral 阶段 runner，映射 openoj-guest-protocol 事件到 domain StageStatus/StageReport，宿主侧 check/aggregate 依据证据形成 Verdict/Score。
- [ ] 测试转绿，不把 guest 自报成功当作终态。

### Task 6: 供应不可变运行时镜像与 SBOM

- [ ] 编写 `infra/runtime-images/algorithm-c/` 供应脚本：下载/构建 kernel、rootfs、guest agent、toolchain，记录来源、内容摘要与 SBOM；摘要/架构不匹配拒绝启动。
- [ ] 在本机 KVM 上真实引导镜像并记录验证证据。
- [ ] 若进程内某语言工具链无法在本机构建，保持单固定 Runtime 并明确 `Unverified`，不声明生产语言矩阵。

### Task 7: 组装 judge node 真实 Firecracker executor

- [ ] 写 fail-closed 配置测试：production+mock 拒绝、强制层缺失拒绝、真实 profile 选择正确 executor。
- [ ] 在 `openoj-judge-core`/judge-node 注入 Firecracker executor（实现 StageExecutor/JudgeExecutor），development mock 保持显式且不可被生产选择。
- [ ] 真实执行路径：领取 → 启动 VMM → guest build/run → 宿主 check → 幂等提交结果；取消在任何阶段收敛并释放 VMM。

### Task 8: 真实 KVM 端到端闭环与故障测试

- [ ] 本机 KVM 上跑通：CLI 提交 → control-plane 派发 → judge-node 启动 Firecracker → guest 编译运行 → 宿主 check → 幂等提交终态。
- [ ] 故障/恶意 workload 测试：CPU 循环、内存膨胀、fork 膨胀、磁盘写满、无限输出、超时、guest hang/panic/reboot、vsock 畸形/超限消息、取消竞态、重复 teardown、宿主重启 reconciler。
- [ ] release 构建 + 固定 workload 记录环境与原始数据；不把 mock 或缺失 KVM 的结果当作生产隔离/性能结论。

### Task 9: 完成仓库门禁与证据

- [ ] 更新架构/威胁/运维/协议/验证事实与 CI；同步 `workspace-and-crates.md`、`overview.md`、`risks.md`、`README.md`。
- [ ] 运行 `cargo fmt --check`、workspace build、Clippy `-D warnings`、全部测试、依赖策略、`check-workspace.py`、`check-docs.sh` 与全部 Skill 校验。
- [ ] 逐逻辑提交（RED→GREEN），正文含 `Refs:`/`Tests:`/`Unverified:`，无 `Co-authored-by:`；普通变更只合入 `dev`。

## 回退与停止条件

出现 guest 逃逸迹象、secret 泄漏、跨任务数据泄漏、资源无限增长、取消后成功计分、供应链摘要不匹配或强制层缺失仍继续执行时，停止新 VMM 写入，保留数据库与 Attempt 历史，按 `SECURITY.md` 上报。宁可回到 ADR 评审，不以放宽隔离或伪造证据继续。
