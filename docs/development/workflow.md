---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: daily development and delivery
references:
  - ../../CONTRIBUTING.md
  - testing.md
  - documentation-style.md
---

# 开发与验证流程

## 任务开始

1. 检查 Git 工作区、当前分支、暂存区和相关 diff。
2. 从 `docs/requirements/` 定位需求和验收 ID。
3. 从 `docs/README.md` 找到对应事实源和 Accepted ADR。
4. 读取 `AGENTS.md` 路由的 Skill。
5. 写明目标、非目标、影响、风险、验证和人工审批点。

如果没有足够信息确定公开语义、安全边界或数据影响，先补 Proposed 文档或请求决策，不以实现细节替代产品决定。

## 日常循环

1. 为缺陷先写可复现回归测试；新功能先确定验收映射。
2. 实现最小完整垂直切片，保持边界内主路径和失败路径同时可用。
3. 运行受影响 crate/package 的快速检查。
4. 运行按变更类型要求的专项检查。
5. 运行 workspace 门禁。
6. 更新事实源和真实验证证据。
7. 审查 diff，列出实际命令、环境、结果和未验证项。

## Phase 0 门禁

```bash
bash scripts/check-docs.sh
```

每个 `skills/*` 还必须通过 `skill-creator` 的 `quick_validate.py`。Skill 创建和更新流程见其自身规范。

## Rust workspace 建立后的默认门禁

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo deny --locked check
```

具体命令在工具链和 workspace 创建时由同一变更验证并更新。不得写入尚不存在或从未执行的“已通过”结果。

## 附加验证

| 变更 | 附加验证 |
|---|---|
| 公开协议/guest-host | 生成物、schema 正反例、未知字段、版本和 conformance |
| Firecracker/jailer | KVM 集成、畸形 guest、资源耗尽、回收、宿主和并发任务存活 |
| Artifact/归档 | 路径穿越、链接、重复项、压缩膨胀、摘要和大小上限 |
| 权限/插件 | allow、deny、scope、quota、审计脱敏和宿主存活 |
| store/migration | 空库、旧版升级、失败、重复执行、数据保留和版本过新拒绝 |
| 调度 | 重复投递、租约过期、节点失联、取消竞态和幂等结果 |
| runtime image | 来源、摘要、SBOM、标准题、恶意 workload 和两种架构适用性 |
| 性能 | release 构建、固定 workload、预热策略、硬件/内核/KVM 和原始数据 |
| 依赖升级 | lock diff、许可证、advisory、来源、feature 和关键行为回归 |

## 交付清单

- 关联需求、验收、风险和 ADR。
- 范围与非范围。
- 安全、协议、数据、插件、运维和许可证影响。
- 自动测试命令与结果。
- 手工验证环境与证据。
- 未验证项及验证条件。
- 所需人工审批。
