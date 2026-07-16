<!-- OPENSPEC:START -->
# OpenSpec Instructions

These instructions are for AI assistants working in this project.

Always open `@/openspec/AGENTS.md` when the request:
- Mentions planning or proposals (words like proposal, spec, change, plan)
- Introduces new capabilities, breaking changes, architecture shifts, or big performance/security work
- Sounds ambiguous and you need the authoritative spec before coding

Use `@/openspec/AGENTS.md` to learn:
- How to create and apply change proposals
- Spec format and conventions
- Project structure and guidelines

Keep this managed block so 'openspec update' can refresh the instructions.

<!-- OPENSPEC:END -->

# 代码审阅约定(codex-review)

完成任何编码改动后,在向用户汇报前,必须调用 codex 对改动做一次代码审阅。

- 工具:已注册的 `codex` MCP server(见 `.mcp.json`),使用其代码审阅能力;审阅范围为**未提交的改动**(staged + unstaged + untracked),等价于 `codex review --uncommitted`。
- 时机:在自检(构建/类型检查/测试)通过之后、汇报结果之前触发。
- 结果处理:
  - 将 codex 审阅发现按严重程度汇总给用户;对合理的问题先修复再汇报,或明确说明为何不修。
  - 若 codex 审阅不可用(未连接/超时),如实告知用户并继续,不要静默跳过。
- 纯文档/配置类改动(不含代码逻辑)可跳过,但需说明已跳过。