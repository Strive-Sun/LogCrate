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

完成任何编码改动后,在向用户汇报前,必须用 Codex 对改动做一次代码审阅。

- 工具:Claude Code 官方插件 [openai/codex-plugin-cc](https://github.com/openai/codex-plugin-cc) 提供的 `/codex:review` 命令(底层走 Codex 原生 review target,上下文独立、比 `codex mcp-server` 省 token)。审阅范围默认即为**未提交的改动**(staged + unstaged + untracked);分支对比用 `/codex:review --base <ref>`。
- 不要再使用 `codex mcp-server`(完整 agent 会话,费 token)。
- 时机:在自检(构建/类型检查/测试)通过之后、汇报结果之前触发。多文件改动建议 `/codex:review --background`,再用 `/codex:status`、`/codex:result` 取结果。
- 结果处理:
  - 将审阅发现按严重程度汇总给用户;对合理的问题先修复再汇报,或明确说明为何不修。
  - 若审阅不可用(插件未装/未登录/超时),如实告知用户并继续,不要静默跳过。
- 纯文档/配置类改动(不含代码逻辑)可跳过,但需说明已跳过。

> 安装:在 Claude Code 中执行 `/plugin marketplace add openai/codex-plugin-cc` → `/plugin install codex@openai-codex` → `/reload-plugins` → `/codex:setup`。