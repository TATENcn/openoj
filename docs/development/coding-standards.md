---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: source code
references:
  - workflow.md
  - testing.md
  - ../architecture/workspace-and-crates.md
  - ../security/threat-model.md
---

# 代码规范

## 通用规则

- 代码、API 和协议标识符使用清晰英文；注释解释不变量、边界和原因，不复述语法。
- 模块保持单一职责，默认私有，公共 API 最小化并记录错误和安全语义。
- 不创建空泛 `utils`、`common` 或 `helpers` 作为跨层依赖逃生口。
- TODO 使用 `TODO(<stable-id>): <reason and completion condition>`；不得用 TODO 替代错误处理、安全检查或测试。
- 生成代码必须来自唯一 schema 和确定命令，不得手工编辑。
- 文本统一 UTF-8 无 BOM；仓库默认 LF，平台脚本按 `.gitattributes`。

## Rust

- 使用仓库固定的 Rust 2024 稳定工具链，通过 rustfmt 和 Clippy `-D warnings`。
- 公共领域值使用语义类型和 newtype，避免跨边界传播无含义 `String`、tuple 或任意 JSON。
- library 使用可分类、有上下文的枚举错误；应用边界负责日志、协议转换和用户可理解呈现。
- 生产代码不得用 `unwrap`、`expect` 或 `panic!` 处理 I/O、解析、网络、数据库、guest、插件、权限和外部输入失败。
- 只有外部输入无法破坏的内部不变量可以断言，并在附近解释不变量来源。
- `unsafe` 限制在最小表达式/模块，每处写可检查的 `// SAFETY:`，再用安全接口封装；调用方不能承担隐藏前提。
- 禁止循环依赖、复制领域类型绕过 crate 边界或通过 re-export 隐藏反向依赖。

## 异步与并发

- 所有 channel、stream、任务集合和队列必须有界。
- 阻塞 I/O、CPU 密集编译协调和外部进程等待不得占用异步执行器核心线程。
- 每个长操作定义超时、取消、关闭和资源回收行为。
- 不跨 `await` 持有不必要锁；不在持锁期间调用插件、网络、存储或 guest。
- 后台任务必须有所有者和 shutdown/join 策略，不创建无人回收的 detached task。
- 修改全局状态、环境变量、时钟或随机源的测试必须隔离，不能污染并行测试。

## 输入与路径

- 在分配或解压前限制消息、集合、字符串、递归深度、文件数和总大小。
- 路径规范化后访问，拒绝绝对路径、父目录、链接逃逸、重复归一化路径和平台特殊路径。
- 权限检查与动作执行位于同一可信边界，避免可被替换的 check/use 两阶段。
- 用户命令不得通过 shell 字符串拼接执行；使用参数数组和固定 executable 映射。

## 错误、日志与指标

- 下层返回上下文，上层责任边界只记录一次，避免重复刷屏。
- 使用结构化 tracing 字段；稳定字段包括 request、evaluation、attempt、stage、node 和 capability ID。
- 不把用户 ID、题目名、路径、源码、错误全文或其他高基数输入用作 metric label。
- 错误和诊断必须有长度上限并脱敏，不泄漏宿主路径、token、隐藏数据或内部堆栈。
- 降级必须返回能力和原因并记录事件，不能静默成功。

## TypeScript/React

- 开启严格类型检查，禁止用 `any` 绕过公开 API 类型。
- API 类型从 schema 生成或通过明确适配器定义，不在组件中复制后端 DTO。
- 权限和数据过滤必须由服务端执行；前端隐藏按钮不是安全控制。
- 异步请求定义取消、加载、空数据、错误和重试状态。
- 用户内容默认转义；富文本和代码渲染使用明确允许列表。
