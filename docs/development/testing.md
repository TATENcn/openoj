---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: testing and validation
references:
  - workflow.md
  - ../requirements/acceptance.md
  - ../security/threat-model.md
  - ../validation/README.md
---

# 测试规范

## 分层

- 单元测试：纯领域规则、状态转换、资源计算和解析边界。
- 集成测试：crate/服务边界、数据库 transaction、协议转换和 Artifact 存储。
- 契约测试：canonical schema、SDK、producer/consumer、guest/host 和插件接口。
- 端到端测试：从 API/CLI 到 microVM、证据和结果持久化。
- 对抗测试：恶意代码、畸形消息、路径、资源耗尽、越权和供应链异常。
- 性能测试：固定环境和 workload 的延迟、吞吐、密度、偏差和回收。
- 混沌/故障测试：worker 崩溃、节点失联、重复投递、存储失败和重启恢复。

## 必须覆盖的行为

每项安全控制至少覆盖允许、拒绝、边界和耗尽。拒绝测试同时断言：

- 返回可分类且脱敏的错误。
- 审计包含稳定决策与原因。
- 宿主和控制平面仍可响应。
- 并发无关任务不被错误终止。
- 临时资源最终回收。

bug 修复必须先建立或同时加入能复现原问题的回归测试。不得仅测试实现细节而没有用户可观察或安全行为断言。

## 确定性

- 测试不依赖公网、用户主目录、执行顺序、真实墙钟或未固定随机数。
- 时间使用可控 clock；随机使用记录 seed；并发测试定义超时。
- 外部服务使用契约 fixture 或隔离测试实例。
- 性能和 KVM 测试可以受环境影响，但必须记录环境并与普通确定性测试分组。

## 测试数据

- fixture 最小化并说明来源，不包含真实用户代码、秘密或受限制题目。
- 恶意样本放在明确目录，默认不执行超出测试 sandbox 的行为。
- 二进制和大型 fixture 使用摘要与生成说明；能确定生成的内容优先由脚本产生。
- 快照测试必须审查语义，不以大范围更新快照掩盖行为变化。

## 失败与例外

- 不删除、skip、放宽断言或延长无限超时来制造绿色门禁。
- flaky test 视为缺陷；隔离必须有 Issue、所有者、原因、影响和到期条件。
- 本机无法运行 KVM、aarch64 或其他目标时写入 `Unverified`，依靠专用 CI/实测，不伪造通过。
