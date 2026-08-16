# OpenOJ

OpenOJ 是一个面向不可信工作负载的开源评测基础设施项目。项目计划使用 Rust 构建控制平面、评测编排、判题节点和 guest agent，以 Firecracker microVM 提供强隔离执行环境，并以算法题作为首个完整评测 Profile。

OpenOJ 的长期目标不是只提供传统 Online Judge 页面，而是建立一套开放、可自托管、可复现、可扩展的通用评测平台：输入可以是代码、文档、图片、音频或工程项目，评测器可以是确定性程序、受控插件、人工流程或经过授权的 AI Agent。

## 当前状态

项目已进入 **P0：单机算法题垂直切片** 的早期实现。当前仓库包含治理与架构基线，以及首个 schema-first 评测内核：它可以校验 `v0alpha1` 请求、维护不同领域身份、编排五个标准阶段，并通过明确标记为非生产的 mock executor 生成结构化结果。仓库尚不能执行用户代码，不具备 Firecracker、数据库、HTTP API 或可发布判题系统。

在许可证决策被接受并添加正式 `LICENSE` 前，本仓库内容不得被视为已获得开源分发授权。参见[许可证治理](docs/governance/licensing.md)。

## 核心原则

- 把所有提交、题目包、检查器、插件和 AI 输入视为不可信数据。
- 把“评测”建模为可版本化、可追踪、可组合的工作流。
- 使用明确的威胁模型和多层隔离，而不是把 Firecracker 当作完整安全方案。
- 对协议、运行时、题目和结果记录完整来源，保证评测可复现。
- 以稳定协议、能力模型和受限插件扩展系统，不向核心进程加载不可信原生代码。
- 设计目标、已实现能力和真实验证结果必须分别标识。
- Agent 必须按照仓库事实源和 Skills 工作，不依赖对话中的临时约定。

## 文档入口

- [文档治理与事实源](docs/README.md)
- [产品愿景](docs/product/vision.md)
- [范围与非目标](docs/product/scope.md)
- [需求基线](docs/requirements/README.md)
- [总体架构](docs/architecture/overview.md)
- [威胁模型](docs/security/threat-model.md)
- [贡献与提交规范](CONTRIBUTING.md)
- [Agent 仓库指令](AGENTS.md)

## 计划中的首个垂直切片

```text
提交代码
  -> 创建版本化评测请求
  -> 调度到 judge node
  -> 启动 Firecracker microVM
  -> guest 内编译和运行
  -> 检查输出并生成证据
  -> 返回结构化评测结果
```

该闭环完成前，不把多模态评测、商业化、复杂组织权限或 AI 自主控制作为 P0 实现目标。
