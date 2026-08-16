---
status: Accepted
owners: OpenOJ security maintainers
last_reviewed: 2026-08-16
applies_to: security documentation
references:
  - threat-model.md
  - trust-boundaries.md
  - ../../SECURITY.md
---

# 安全文档入口

OpenOJ 处理设计上可能恶意的代码和产物。任何“安全”“隔离”“可信”结论都必须关联威胁、控制、拒绝路径测试和目标环境证据。

- `threat-model.md` 定义攻击者、资产、威胁、控制和残余风险。
- `trust-boundaries.md` 定义控制平面、judge host、VMM、guest、插件和外部系统之间允许的数据流。
- `SECURITY.md` 定义漏洞报告和响应原则。

新增宿主权限、网络、设备、文件共享、插件能力、长期凭证或执行器时，必须同时复核两份安全事实源，并使用 `skills/change-openoj-sandbox/SKILL.md`。
