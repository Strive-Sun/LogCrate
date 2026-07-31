# 开发协作流程

本文档记录 LogCrate 在使用 AI 助手(Claude)+ OpenSpec + Codex 审阅时的标准往返流程。

Codex 审阅通过 Claude Code 官方插件 [openai/codex-plugin-cc](https://github.com/openai/codex-plugin-cc) 的 `/codex:review` 命令完成(底层走 Codex 原生 review target,上下文独立、省 token),不再使用 `codex mcp-server`。

## 核心原则

- **规格先行**:功能/架构/破坏性改动先有 OpenSpec change 提案,再编码。
- **编码后必审**:任何代码改动完成后,在汇报前调用 codex 审阅未提交改动。
- **人工确认闭环**:每个 change 由用户实测确认后,才存档 openspec 并提交。

## 单个 OpenSpec change 的往返流程

以下步骤对每个 active change 依次执行,一次只推进一个 change:

1. **提交前置改动**
   开工前确保工作区干净;把上一轮已确认的改动提交,避免混入。

2. **编码**
   按 change 的 `proposal.md` / `design.md` / `tasks.md` 实现,逐项完成。

3. **自检**
   - 后端改动:`cargo check`(必要时 `cargo test`)
   - 前端改动:`tsc --noEmit` + `npm run build`
   - 自检失败先修复,不带病提交审阅。

4. **codex 审阅未提交改动**
   - 运行 `/codex:review`(codex-plugin-cc 提供),默认审阅**未提交的改动**(staged + unstaged + untracked);多文件改动用 `/codex:review --background` + `/codex:status` + `/codex:result`。
   - 将发现按严重程度汇总;对合理问题先修复再汇报,或说明为何不修。
   - 若审阅不可用(插件未装/未登录/超时),如实告知并继续,不静默跳过。

5. **汇报**
   向用户说明:实现了什么、自检结果、codex 审阅结论与处理。

6. **等待用户实测**
   用户在真实环境验证。**在收到用户确认前,不存档、不提交。**

7. **存档 + 提交**(收到用户确认后)
   - 将 change 的 `tasks.md` 全部标记为 `- [x]`(反映真实完成状态)。
   - `openspec archive <change-id> --yes` 归档,更新 `specs/` 基线。
   - `openspec validate --strict` 确认基线通过。
   - 提交 commit(含代码改动与 openspec 归档)。

8. **进入下一个 change**,回到步骤 1。

## 当前待推进的 change 顺序

1. `add-backend-suffix-filter` — 后端通知应用后缀筛选(小、修实际 bug)
2. `add-index-progress-events` — 后台索引 + 进度事件 + 边建边读
3. `add-encoding-support` — 编码检测(GB18030/UTF-16BE)+ 手动指定
4. `refactor-zip-streaming-read` — zip 条目真流式读取,消除整条目入内存
5. `add-backend-unit-tests` — 后端单元测试覆盖
6. `add-frontend-lint-tooling` — 前端 lint/format 工具链

> 顺序可按需调整;后端功能类先行,测试与工具链收尾。

## 相关约定

- Codex 审阅约定与插件安装步骤见 `CLAUDE.md`「代码审阅约定」。
- OpenSpec 规范见 `openspec/AGENTS.md`。
- git 安全:默认不改 `main` 直推;提交只在用户明确要求时进行。

## 发布版本号

- 应用版本的唯一人工维护来源是 `src-tauri/Cargo.toml` 中的 `[package].version`。
- `tauri.conf.json` 不重复声明版本；Tauri 2 会自动使用 Cargo package version。
- 根目录 npm package 是私有前端工程,不声明发布版本。
- 修改 `Cargo.toml` 后运行 `cargo check`,由 Cargo 自动同步 `Cargo.lock`,不要手工修改 lockfile。

### 发布步骤

1. 在 `src-tauri/Cargo.toml` 中修改 `[package].version`。
2. 在 `CHANGELOG.md` 中新增 `## [版本号] - YYYY-MM-DD` 章节。
3. 按“新增 / 优化 / 修复 / 工程质量”等类别组织内容；没有内容的类别可以省略。
4. 每项变化单独写一条 `- ` bullet,描述具体的用户可见变化或工程改进；禁止使用“修复若干问题”“待补充”等模糊占位描述。
5. 将 `Unreleased` 中已完成的条目移动到新版本章节,不要在两个章节重复保留。
6. 运行 `cargo check` 更新 `Cargo.lock`。
7. 运行 `npm run release:check`,确认 Cargo 版本、CHANGELOG 版本章节及更新列表一致。
8. 提交版本变更后创建 `v版本号` tag 并推送。

Release 工作流会再次校验 tag、Cargo 版本与 CHANGELOG 章节一致,并自动把该版本的逐条更新内容写入 GitHub Release Notes；任一项缺失或不一致都会停止发布。

### 自动更新签名

Tauri updater 会拒绝任何未通过签名验证的更新包。签名公钥保存在 `src-tauri/tauri.conf.json`,私钥不得进入仓库、应用包或构建日志。

首次配置或需要重建发布环境时:

1. 运行 `npm run tauri signer generate -- --ci -w ~/.tauri/logcrate-updater.key` 生成密钥对；生产密钥建议通过 `--password` 设置密码。
2. 将 `.pub` 文件的完整内容填写到 `tauri.conf.json` 的 `plugins.updater.pubkey`。
3. 在 GitHub 仓库的 Actions secrets 中配置:
   - `TAURI_SIGNING_PRIVATE_KEY`:私钥文件的完整内容。
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`:生成私钥时设置的密码；无密码密钥可留空。
4. 将私钥和密码备份到受控的密码库。丢失私钥后,已安装版本将无法验证任何新密钥签发的更新。

发布工作流会在构建前检查私钥 secret,随后生成并上传 `latest.json`、更新包和 `.sig` 签名。Windows 更新优先使用 NSIS,macOS 使用签名的 `.app.tar.gz`。

### Cloudflare Pages 更新镜像

Windows 自动更新同时使用固定 Pages 项目 `logcrate-updates`。Pages 只镜像 `latest.json` 和小于 25 MiB 的 NSIS 更新包；macOS 更新包及 Windows MSI 继续使用 GitHub Release。镜像不会改变 updater 签名，客户端仍只信任 `tauri.conf.json` 中的现有公钥。

首次配置时，由维护者在 Cloudflare 创建仅限目标账户、具有 Cloudflare Pages 编辑权限的 API Token，然后在 GitHub 仓库的 Actions secrets 中添加：

- `CLOUDFLARE_API_TOKEN`：最小权限 Pages token，值不得写入仓库、issue、构建参数或日志。
- `CLOUDFLARE_ACCOUNT_ID`：Pages 项目所在账户的 account ID。

不要把 secret 值发送给 AI 助手。工作流只读取 secret 并通过 Cloudflare API 确认 `logcrate-updates` 项目及其 production branch；缺少配置或无权访问时会在构建和公开 Release 前失败。

发布采用以下顺序，不能手工跳过中间步骤：

1. Windows 与 macOS job 把签名产物写入同一个 draft GitHub Release。
2. 汇总 job 下载并验证所有清单引用的资产、签名、版本和 Windows NSIS 25 MiB 边界。
3. Pages production 先部署不含 `latest.json` 的 fallback-only 状态并验证公开 URL 返回 404。
4. GitHub draft 转为公开 Release；这个短暂窗口内客户端会从 Pages 404 顺序回退 GitHub。
5. 同一次 Pages deployment 上线版本化 Windows 包和新 `latest.json`，随后从公网核对清单、缓存头和包 SHA-256。

如果第 3 步之前失败，GitHub Release 保持 draft。如果 GitHub 已公开但完整 Pages 部署或验证失败，失败恢复步骤会再次部署并验证 fallback-only，workflow 以失败结束，客户端仍可使用 GitHub。恢复时修复原因后在 GitHub Actions 中只重新运行失败的 `finalize` job；它同时接受 draft 或已公开的同 tag Release。不得手工恢复旧版本 `latest.json`，否则 Tauri 会在第一个有效旧清单停止而看不到 GitHub 新版本。

`v1.0.21` 及更早版本没有 Pages endpoint，仍需先从 GitHub 更新到首个双 endpoint 版本。
