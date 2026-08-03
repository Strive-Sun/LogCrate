---
name: logcrate-workflow
description: Enforce the LogCrate repository workflow by loading its rule manual, recovering current work state, selecting the required OpenSpec path, and applying the repository's execution, validation, and Git gates. Use for every task performed in the LogCrate repository, including questions, investigations, reviews, diagnoses, documentation, implementation, testing, releases, and resumed work.
---

# LogCrate Workflow

Use this Skill as the entry point to LogCrate's repository-owned instructions. Keep the files under `规则手册/` as the only authoritative rule text; do not copy their detailed requirements into this Skill.

## Enter the workflow

1. Locate the LogCrate repository root by finding the `AGENTS.md` that points to `规则手册/`.
2. Read `AGENTS.md` completely.
3. Read these files completely and in this exact order before starting or continuing the task:
   1. `规则手册/Step-0/OPENSPEC.md`
   2. `规则手册/Step-1/项目配置.md`
   3. `规则手册/Step-2/工作约束.md`
   4. `规则手册/Step-3/工作状态.md`
   5. `规则手册/Step-4/执行规则.md`
   6. `规则手册/Step-5/验证规则.md`
4. Apply the OpenSpec decision made by Step-0. Read `openspec/AGENTS.md` and the relevant specs or changes only when Step-0 requires them.
5. Reconcile Step-3 with the current task documents, `git status`, relevant diffs, and recent Git history before making changes. Treat Step-3 as a dynamic recovery record, not as proof that work is complete.

## Execute the task

- Treat `AGENTS.md`, the six rule files, applicable OpenSpec documents, and repository facts as authoritative within their stated responsibilities.
- Preserve user-owned and unrelated worktree changes.
- Follow the smallest-task sequencing, validation level, evidence, handoff, and local commit requirements defined by Step-2, Step-4, and Step-5.
- Do not use this Skill to bypass proposal approval, required human or real-device validation, task ordering, or Git gates.
- Do not automatically push, tag, publish, archive a change, or modify remote state unless the user explicitly authorizes that action.

## Handle conflicts

Perform read-only checks when instructions, specifications, state records, configuration, code, or Git history disagree. Stop affected mutations and ask the maintainer when the conflict cannot be resolved using the responsibility boundaries defined by the rule manual.

This Skill supplements repository discovery and does not replace `AGENTS.md` or any file under `规则手册/`.
