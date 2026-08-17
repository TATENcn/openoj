---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: project risk register
references:
  - overview.md
  - ../security/threat-model.md
  - ../governance/licensing.md
---

# 风险清单

| ID | 风险 | 当前缓解 | 状态 |
|---|---|---|---|
| RISK-SEC-001 | 把 Firecracker 误当完整沙箱，忽略宿主、jailer 和资源边界 | 威胁模型、纵深隔离、专项 Skill 和恶意 workload 验收 | Open |
| RISK-REPRO-001 | 共享硬件和工具链漂移导致判题不公平或不可复现 | 不可变 Runtime、CPU/负载策略、环境来源和重复 benchmark | Open |
| RISK-PROTOCOL-001 | 过早冻结通用协议，无法容纳第二种评测 Profile | P0 使用 alpha；两个 Profile 与 conformance 通过后再稳定 | Open |
| RISK-SCOPE-001 | 多模态、AI、商业化设想使 P0 无法交付 | `scope.md` 非目标和分阶段验收 | Mitigated |
| RISK-PLUGIN-001 | 高自由度插件绕过安全或拖垮宿主 | capability Broker、Wasm/服务/microVM 分级、默认拒绝 | Open |
| RISK-SUPPLY-001 | kernel、rootfs、编译器和依赖被投毒或许可证不兼容 | 摘要、来源、SBOM、签名、许可证门禁 | Open |
| RISK-DATA-001 | 源码、隐藏测试和日志泄漏 | 分类、最小访问、脱敏、保留期和审计 | Open |
| RISK-OPS-001 | KVM/网络/磁盘操作增加 judge node 运维复杂度 | 明确部署 Profile、节点排空、watchdog 和 runbook | Open |
| RISK-DB-001 | migration、连接池或错误回滚导致控制状态不可用、版本不兼容或数据损坏 | 显式 migrate-before-start、schema 版本拒绝、有界连接池、事务测试、备份与非破坏回滚 runbook | Open |
| RISK-AGENT-001 | Agent 根据模糊指令扩大范围或修改高风险策略 | 事实源、Skills、人工审批、Git 授权和停止条件 | Mitigated |
| RISK-LEGAL-001 | 无许可证或第三方权利不清阻止开源/商业分发 | 发布阻塞、许可证 ADR、DCO/CLA 和内容权利分类 | Open |

获得新证据或发现新风险时必须在相关变更中更新本表。`Mitigated` 不表示风险消失，只有控制已建立。
