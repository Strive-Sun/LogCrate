# Change: 建立前端 lint/format 工具链

## Why
前端目前没有 Prettier/ESLint 配置与 lint 脚本,CI 仅有 `tsc --noEmit` 与 build,代码风格与低级错误缺乏自动守护。基线脚手架任务原含"前端 Prettier/lint"与"引入 Radix 基元",前者应补齐;后者按当前实际(手写轻量组件)重新定位为可选,不强制引入。

## What Changes
- 引入 Prettier 配置与 `format`/`format:check` 脚本
- 引入 ESLint(TypeScript + React Hooks 规则)与 `lint` 脚本
- CI 增加 lint 与 format 检查步骤
- 明确 UI 基元策略:沿用手写轻量组件,暂不引入 Radix;若未来交互复杂度上升再评估

## Impact
- Affected specs: `frontend-tooling`(新增能力)
- Affected code:
  - `package.json`(devDependencies + scripts)
  - `.prettierrc` / `.eslintrc`(或等价配置)
  - `.github/workflows/ci.yml`(lint/format 步骤)
