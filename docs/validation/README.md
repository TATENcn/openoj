---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: validation evidence
references:
  - ../requirements/acceptance.md
  - ../development/testing.md
---

# 验证证据

本目录保存可审查验证摘要；大型原始日志、镜像和二进制保存在受控 Artifact 系统，文档记录位置与内容摘要。

每条记录必须包含：

- 日期、commit 和关联需求/验收/威胁 ID。
- 操作系统、内核、CPU、内存、虚拟化和负载条件。
- Firecracker、kernel、rootfs、guest agent 和 Runtime 版本/摘要。
- debug/release 构建和完整命令。
- 输入 workload、预热状态、重复次数和采样方法。
- 结果、原始数据位置、摘要、异常和未验证项。

设计目标不得写入此目录冒充结果。新证据不删除旧证据；过期记录标记替代关系和适用范围。

## 记录索引

- `2026-08-16-phase-0-governance.md`：Phase 0 文档与 Agent 治理基线。
- `2026-08-16-p0-evaluation-kernel.md`：P0-A schema-first 内存评测内核。
- `2026-08-17-p0b-durable-control-spine.md`：P0-B PostgreSQL 持久化控制脊柱。
