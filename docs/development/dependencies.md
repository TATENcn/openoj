---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: dependencies and supply chain
references:
  - ../architecture/technology-stack.md
  - ../governance/licensing.md
  - ../security/threat-model.md
---

# 依赖与供应链规范

- 直接依赖统一声明并由 lockfile 固定；应用和工具使用 `--locked` 门禁。
- 关闭不需要的 default features，解释大型、原生、网络、密码学和 `unsafe` 依赖的 feature。
- 禁止 Git 分支、未校验下载地址或来源不明二进制成为生产依赖。
- 新依赖记录用途、维护状态、许可证、来源、已知 advisory、替代方案和移除成本。
- 密码学、签名、压缩、解析和虚拟化不得自创未经审查的基础算法。
- kernel、rootfs、编译器和 runtime image 与普通库同样纳入来源、摘要、SBOM 和漏洞响应。
- 依赖升级独立成变更，保留 lock diff，运行关键 API、协议和安全回归。
- 发布前的允许许可证、DCO/CLA 和再分发规则以 Accepted 许可证 ADR 为准；当前许可未决是发布阻塞项。
