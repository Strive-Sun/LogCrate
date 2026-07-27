# AI Task Execution Rules

Before starting or continuing any task, AI assistants MUST read these documents completely in order:

0. `@/规则手册/Step-0/OPENSPEC.md` — determine whether the request requires the OpenSpec workflow.
1. `@/规则手册/Step-1/项目配置.md` — establish LogCrate architecture, design context, commands, and repository map.
2. `@/规则手册/Step-2/工作约束.md` — determine when work state must be persisted and how it closes with validation and Git.
3. `@/规则手册/Step-3/工作状态.md` — check unfinished work, repository state, and recovery context.
4. `@/规则手册/Step-4/执行规则.md` — apply LogCrate task sequencing, implementation, and recovery rules.
5. `@/规则手册/Step-5/验证规则.md` — apply testing, boundary, gate, failure-handling, and evidence rules.

All files in `@/规则手册/` are specific to LogCrate. If these instructions conflict with repository facts or cannot be reconciled, stop and ask the maintainer before implementation.

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
