---
status: Accepted
owners: OpenOJ maintainers
last_reviewed: 2026-08-16
applies_to: Markdown and normative documentation
references:
  - ../README.md
  - ../../.editorconfig
  - ../../.gitattributes
---

# 文档编写规范

## Frontmatter

`docs/**/*.md` 必须声明 `status`、`owners`、`last_reviewed`、`applies_to` 和 `references`。日期使用 `YYYY-MM-DD`。根 README、贡献、安全、行为准则和 Agent 指令不要求 frontmatter。

## 内容

- 先写结论和边界，再写背景。
- 一个文档定义一个领域事实，重复内容改为链接。
- 使用稳定 ID 和规范性语言；示例不得暗示未实现能力。
- 命令必须可复制，结果与命令分离，未执行的命令不得标记通过。
- 图表达主路径和边界，异常、重试、取消和降级由正文说明。
- 目标值、设计预期、自动测试和真实环境验证必须分开。
- 链接使用相对路径并可从当前文件解析；重命名同步反向引用。

## 语言和格式

- Phase 0 中文为规范事实源；代码、协议字段和 Commit subject 使用英文。
- 同一文档术语必须与 `docs/product/glossary.md` 一致。
- 文本使用 UTF-8 无 BOM；默认 LF；文件以单个换行结束。
- Markdown 标题逐级递进，列表和代码块前后保留空行。
- 不使用大段加粗、装饰性 emoji 或勾选框表示验证结论。

## 验证记录

验证记录必须包含日期、commit、操作系统/内核、CPU/内存/KVM、Firecracker/kernel/rootfs、构建类型、命令、输入负载、结果、原始产物位置和未验证项。过期结论保留并指向新记录，不静默覆盖历史。
