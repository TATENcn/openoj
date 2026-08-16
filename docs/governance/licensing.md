---
status: Proposed
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: licensing and distribution
references:
  - ../../README.md
  - ../architecture/risks.md
---

# 许可证治理

OpenOJ 计划成为开源开放项目并保留未来商业化选择，但当前尚未选择许可证。没有正式 `LICENSE` 时，仓库内容不得被描述为已经授予复制、修改或再分发权利。

## 必须分别决策的对象

- 平台源代码。
- SDK、协议 Schema 和示例。
- Firecracker kernel、rootfs 和语言 runtime images。
- 题面、测试数据、标准答案和媒体内容。
- 用户提交及评测产物。
- 第三方插件和插件市场元数据。
- 商标、名称和视觉资产。

## 决策要求

正式分发前必须通过 ADR：

1. 比较 Apache-2.0、MIT、AGPL 等实际候选及商业、专利、贡献和分发影响。
2. 决定 DCO 或 CLA，并明确 `Signed-off-by` 策略。
3. 定义依赖和 runtime image 的许可证允许/拒绝列表。
4. 定义问题内容和用户提交的权利条款。
5. 定义 OpenOJ 名称和兼容实现的商标规则。
6. 由具备相应权限的人类维护者接受；Agent 不得代替法律决策。

## 当前门禁

- 不创建正式 release 或公开分发镜像。
- 不把仓库标记为某个许可证。
- 不接受权利来源不清晰的复制代码、题目或二进制产物。
- 第三方依赖仅可用于研究和原型，并记录来源与许可证待审状态。
