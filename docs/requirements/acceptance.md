---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: phase and product acceptance
references:
  - functional.md
  - non-functional.md
  - ../product/roadmap.md
---

# 验收基线

## Phase 0：治理与文档

### ACC-F0-001：事实源可发现

一个未读取历史对话的 Agent 能从根 `AGENTS.md` 找到需求、架构、安全、开发、Git 和 Skill 事实源，且不存在相互冲突的重复规则。

### ACC-F0-002：稳定需求映射

首个垂直切片的功能、安全、可靠性、可复现和维护需求具有稳定 ID，并能映射到后续测试与 Commit。

### ACC-F0-003：安全边界基线

威胁模型覆盖用户提交、题目工具、插件、guest、judge node、控制平面、供应链和 AI 输入，记录信任边界、控制与残余风险。

### ACC-F0-004：协作门禁

仓库明确分支、Commit、PR、人工审批、文档状态、ADR 和验证证据规则，并提供 PR/Issue 模板。

### ACC-F0-005：Skills 可加载

首批 Skills 具有合法 frontmatter、明确触发 description、事实源读取步骤、验证与停止条件，并通过 `quick_validate.py`。

### ACC-F0-006：文档自动检查

`bash scripts/check-docs.sh` 能检查 Phase 0 文档的 frontmatter、状态、Markdown 与 frontmatter `references:` 相对链接、CRLF/BOM 和空文件，并在 CI 中执行。

## P0：单机算法评测

### ACC-P0-001：端到端闭环

给定固定 Problem Version、Runtime 和 Submission，系统能经 API/CLI 创建 Evaluation，在 Firecracker guest 内构建和运行，检查至少一个测试用例，并返回结构化结果和来源信息。

### ACC-P0-002：资源耗尽隔离

CPU 循环、内存膨胀、fork/线程膨胀、磁盘写满、无限输出和超时 workload 被按策略终止；宿主、控制平面和并发 Evaluation 保持可用，资源在限定时间内回收。

### ACC-P0-003：可靠重试

在排队、启动 microVM、运行和结果提交阶段模拟进程崩溃或重复投递，不丢失 Evaluation、不重复计分，并保留 Attempt 历史。

### ACC-P0-004：可复现结果

同一固定输入和环境多次执行得到相同 Verdict/Score；任何资源指标差异均在接受范围内并记录宿主条件。

### ACC-P0-005：默认无网络与无秘密

guest 无法访问未授权网络、宿主路径和平台凭证；拒绝被审计，且错误不泄漏敏感值。

### ACC-P0-006：故障可观测

从一个外部 request ID 能定位 Evaluation、Attempt、Stage、judge node、microVM 和结构化错误，而无需搜索用户源码或高基数字段。

### ACC-P0-007：性能基线可复现

使用 release 构建和固定 workload 采集 cold/warm 启动、排队、build/run、结果持久化、吞吐和回收的 P50/P95/P99，记录宿主硬件、内核、KVM、Firecracker、镜像摘要、预热方法、重复次数和原始数据位置。

### ACC-P0-008：运行时供应链可追踪

一个历史 Evaluation 能解析到不可变 Runtime、guest agent、kernel 和 rootfs 摘要；对应构建来源、依赖锁定、许可证状态和 SBOM 可追踪，摘要或架构不匹配时执行被显式拒绝。

### ACC-P0-009：Artifact 内容寻址与消息有界

源码、测试数据、日志和二进制通过媒体类型、大小和内容摘要引用；摘要不匹配被拒绝，调度消息只携带有界元数据/引用，不嵌入无上限 Artifact 内容。

### ACC-P0-010：阶段与证据完整

一个算法 Evaluation 明确产生 prepare、build、run、check 和 aggregate 阶段；每阶段保存状态、起止/资源摘要、有界诊断和 Evidence/Artifact 引用，阶段失败不会伪造后续成功。

### ACC-P0-011：关键操作审计完整

安全拒绝、权限决定、调度、Rejudge 和人工覆盖分别产生结构化审计，包含主体、动作、目标、决策、原因和关联 ID；敏感内容被脱敏且记录不能由业务结果静默覆盖。AI 工具调用在 P4 由 `ACC-P4-001` 验收。

### ACC-P0-012：领域身份与不可变版本

Problem、Problem Version、Submission、Evaluation 和 Attempt 使用不同稳定身份；修改题面、数据、限制、计划或评分创建新 Problem Version，重试只增加 Attempt，Rejudge 保留旧结果并关联新版本来源。

### ACC-P0-013：版本化请求与结果结构

EvaluationRequest 和 EvaluationResult 按 alpha schema 验证，明确引用 Problem Version、Submission、Runtime、Plan/Policy、Attempt、Stage、Verdict/Score、指标、诊断和 Evidence；缺失安全关键字段、未知成功语义或超限消息被拒绝。

### ACC-P0-014：不可信输入族拒绝路径

分别对 Submission、题目包、checker/interactor/generator、归档和外部响应执行畸形、超限或越权测试；每类输入在进入领域、路径、命令或权限边界前被验证，拒绝不影响宿主存活。AI 参数在 P4 由 `ACC-P4-001` 验收。

### ACC-P0-015：纵深执行配置

生产 Profile 可证明同时启用 KVM/Firecracker、jailer 或等价 chroot/uid/gid/namespace/cgroup 约束、seccomp、资源上限和独立 watchdog；缺失任一强制层时配置 fail closed，不能静默切换 mock/container executor。

### ACC-P0-016：全系统资源有界

对队列、消息、日志、输出、Artifact、定时器、并发和重试分别验证配置上限、背压/超限错误、取消和清理；无限增长 workload 不造成宿主内存、磁盘或任务永久泄漏。

### ACC-P0-017：取消状态机与竞态

分别在排队、任务租约、microVM 启动、build/run、结果提交和终态后发起取消；状态转换、重复取消、取消/完成竞态、审计和资源回收具有确定结果，不把已取消任务错误计分或遗留执行资源。

### ACC-P0-018：写边界幂等

对外写 API、Submission/Evaluation/Attempt 创建、任务投递、Artifact 注册和结果提交分别使用稳定幂等键；重复、超时重试、进程崩溃后重放不会创建重复领域对象、覆盖不相关 Artifact 或重复计分。

### ACC-P0-019：外部与管理能力默认拒绝

未显式授予的外部服务访问、管理操作、宿主文件和高风险能力被服务端拒绝并审计；授权包含主体、scope、期限和配额，UI 隐藏或管理员身份不能替代服务端检查。

P0 的具体性能阈值必须在原型 benchmark 后新增 Accepted 验收项；当前不以猜测数字通过性能验收。

## P3：协议与插件稳定

### ACC-P3-001：插件能力隔离

至少一个外部插件只通过声明的 capability、scope、配额和版本化契约工作；未授权网络、存储、宿主句柄和超额调用被拒绝、审计且不影响宿主存活。

### ACC-P3-002：两个 Profile 的协议一致性

算法题和至少一个非算法 Profile 使用同一 Submission、Evaluation、Stage、Artifact、Evidence 和 Result 核心模型，独立实现通过共享 conformance suite，未知 Profile 显式能力协商或拒绝。

### ACC-P3-003：版本与滚动兼容

对公开协议、guest/host 和插件契约声明的每个版本组合执行 producer/consumer、升级、降级、未知字段/值和不支持版本测试；破坏性变化遵循 Accepted ADR、弃用和迁移规则。

## P4：受控 AI 评测

### ACC-P4-001：AI 最小权限与可复核

AI 只能通过强类型、最小权限、配额和审计工具访问任务所需数据；提示词注入不能扩大权限，高风险操作需要人工批准，模型输出保留来源、置信/限制和可复核证据，不能静默覆盖确定性结果。
