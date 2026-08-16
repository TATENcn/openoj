---
status: Accepted
owners: OpenOJ security maintainers
last_reviewed: 2026-08-16
applies_to: platform threat model
references:
  - README.md
  - trust-boundaries.md
  - ../requirements/non-functional.md
  - ../architecture/risks.md
---

# 威胁模型

## 安全目标

OpenOJ 的首要安全目标是限制不可信工作负载、题目工具、插件和外部输入对宿主、控制平面、其他租户、秘密、隐藏测试与结果完整性的影响。系统不承诺消除所有硬件侧信道或未知 KVM/内核漏洞，但必须纵深限制单点失效的影响范围。

## 受保护资产

- 控制平面身份、权限、策略和数据库完整性。
- 平台凭证、发布密钥、对象存储授权和审计记录。
- judge host、其他 microVM、调度容量和基础设施可用性。
- 用户源码、私有 Artifact、隐藏测试和标准答案。
- Problem Version、Runtime、Evaluation Result 和 Rejudge 来源完整性。
- kernel、rootfs、guest agent、编译器、插件和发布供应链。

## 攻击者与不可信来源

- 匿名或认证用户提交恶意代码、压缩包、参数或请求序列。
- 题目作者上传恶意 checker、interactor、generator、测试数据或路径结构。
- 插件作者请求过量能力或利用宿主接口。
- 外部服务返回畸形、超大、重放或注入内容。
- AI 输入通过 prompt injection 诱导工具越权。
- 被攻陷或配置错误的 judge node 伪造状态和结果。
- 具有部分内部权限的操作者误用或恶意访问数据。
- 供应链攻击者污染依赖、构建工具、kernel、rootfs、runtime 或发布产物。

## 信任假设

- 宿主 CPU、KVM 和受支持内核在已知安全基线内，并按计划更新。
- 控制平面与 judge node 的身份和传输密钥由外部可信基础设施正确提供。
- PostgreSQL 与对象存储本身可用且访问策略正确，但其返回数据仍需完整性和 Schema 校验。
- 被标记为可信的构建产物经过摘要、来源、签名和策略验证。

这些是假设，不是系统自行证明的事实；部署偏离时不能复用对应安全结论。

## 威胁与控制

### THR-HOST-001：guest 逃逸或宿主权限提升

攻击者利用 guest kernel、virtio、VMM、KVM 或宿主配置缺陷访问 judge host。

控制：

- `CTL-ISOLATION-001`：生产使用 Firecracker microVM 和受支持 KVM/内核。
- `CTL-JAIL-001`：使用 jailer 或等价的 chroot、namespace、cgroup、uid/gid 和最小权限约束。
- `CTL-SECCOMP-001`：启用 Firecracker 默认 seccomp，并审查自定义设备/功能引入的 syscall。
- `CTL-HOST-001`：控制平面与生产 judge host 分离；judge host 不保存长期平台秘密。
- `CTL-WATCHDOG-001`：独立 watchdog 检测并强制终止失控 VMM。

验证必须包括 guest 内提权/设备探测、畸形 vsock、VMM 失联和宿主/并发任务存活。

### THR-RESOURCE-001：资源耗尽和拒绝服务

攻击者消耗 CPU、内存、进程、磁盘、I/O、网络、日志、输出、队列、Artifact 或重试预算。

控制：

- `CTL-RESOURCE-001`：对 CPU、内存、进程、墙钟、磁盘、I/O、输出、Artifact、队列和调用频率设置有界配额。
- `CTL-BACKPRESSURE-001`：所有跨线程/进程队列有容量、超时、取消和关闭语义。
- `CTL-OUTPUT-001`：stdout/stderr/serial/log 使用截断、环形或其他上界存储。
- `CTL-RECLAIM-001`：失败、取消和租约过期触发幂等回收；残留资源由 reconciler 清理。

验证覆盖允许、边界、超限、并发竞争、重复终止和回收时限。

### THR-DATA-001：秘密、源码或隐藏测试泄漏

攻击者通过 guest、错误、日志、Artifact URL、缓存、快照、跨任务磁盘或插件读取敏感数据。

控制：

- `CTL-SECRET-001`：guest 不接收平台长期凭证，只使用任务范围的最小数据通道。
- `CTL-STORAGE-001`：Artifact 访问使用短期、范围受限授权或宿主代理，并校验摘要。
- `CTL-SANITIZE-001`：日志和错误在责任边界脱敏；不记录原始敏感内容。
- `CTL-ERASE-001`：临时盘、快照恢复状态和任务目录不得跨租户复用未清理数据。
- `CTL-CACHE-001`：缓存键包含所有安全和版本维度，缓存内容按敏感级别隔离。

### THR-INTEGRITY-001：结果、运行时或证据伪造

被攻陷节点、重放消息或存储篡改产生无法追踪的 Verdict/Score。

控制：

- `CTL-PROVENANCE-001`：结果关联 Evaluation、Attempt、Stage、Problem Version、Runtime、节点和 Artifact 摘要。
- `CTL-IDEMPOTENCY-001`：Attempt 和结果提交使用幂等键、租约和状态转换校验。
- `CTL-ATTEST-001`：生产产物按供应链策略验证摘要和签名；节点身份经过认证。
- `CTL-AUDIT-001`：Rejudge、人工覆盖、策略和 AI 工具调用生成不可静默覆盖的审计事件。

### THR-PROTOCOL-001：畸形、超大或版本不兼容消息

攻击者利用解析深度、长度、整数、未知 enum、重复字段、路径或状态机差异触发崩溃或绕过。

控制：

- `CTL-PARSE-001`：在分配前限制消息、集合、字符串、嵌套、Artifact 和输出大小。
- `CTL-VERSION-001`：协议版本与 capability 显式协商，不支持时 fail closed。
- `CTL-STATE-001`：状态转换集中验证，拒绝跳跃、回退和重复终态。
- `CTL-PATH-001`：归档和路径规范化后访问，拒绝绝对路径、`..`、链接逃逸、重复项和解压膨胀。

### THR-PLUGIN-001：插件越权或拖垮宿主

控制：插件默认无能力；所有 host call 通过 capability Broker；调用有超时、配额、取消和审计；高风险 evaluator 在 microVM 或隔离服务运行；核心进程不加载不可信原生动态库。

### THR-SUPPLY-001：供应链投毒

控制：固定工具链与 lockfile；限制依赖来源；记录许可证；kernel/rootfs/runtime 使用可复现流程、摘要、SBOM 与签名；发布需要来源证明；紧急漏洞升级保留回归证据。

### THR-AI-001：提示词注入和工具越权

控制：模型只接收任务所需数据；工具使用强类型参数和服务端权限；高风险写操作需人工批准；模型文本永不直接成为命令、SQL 或权限决策；所有调用关联主体和审计。

### THR-INSIDER-001：内部权限误用

控制：职责分离、最小权限、敏感操作审批、不可静默覆盖审计、短期访问、环境隔离和定期权限复核。管理员身份不是跳过 Artifact 分类和日志脱敏的理由。

## 明确不做的安全声明

- 不声明 Firecracker 能消除所有侧信道或未知逃逸。
- 不声明开发 mock executor 具有生产隔离。
- 不声明同一宿主不同负载具有完全确定的 CPU 时间。
- 不声明 CI 通过等于生产环境安全验证。
- 不允许以“仅管理员可用”为理由省略输入校验和审计。

## 复核触发

新增设备、网络、文件共享、快照复用、缓存、宿主能力、插件接口、AI 工具、身份模型、执行器或生产部署方式时必须复核本文。发现新威胁时分配稳定 `THR-*`/`CTL-*` ID，并更新验收与风险。
