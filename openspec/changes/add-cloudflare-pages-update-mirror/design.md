## Context

当前客户端只读取 `https://github.com/Strive-Sun/LogCrate/releases/latest/download/latest.json`。Tauri updater 会按配置顺序尝试 endpoint，但第一个有效清单即使版本较旧，也不会继续比较后续 endpoint。因此，普通的“发布 GitHub 后尽力同步 Pages”会产生旧镜像遮住新版本的风险。

Cloudflare Pages 项目 `logcrate-updates` 已部署到 `https://logcrate-updates.pages.dev/`。当前 Windows NSIS updater 包约 9.36 MiB，适合 Pages；当前 macOS updater 包约 24.11 MiB，距离 Pages 25 MiB 单文件上限过近，不适合纳入镜像。

## Goals / Non-Goals

- Goals:
  - 中国大陆 Windows 用户可优先从 Pages 检查并下载签名更新。
  - Pages 不可访问、未配置或部署失败时，客户端可顺序回退到官方 GitHub endpoint。
  - 不允许旧 Pages 清单遮住已经公开的 GitHub 新版本，也不允许清单先于其引用的 Windows 包上线。
  - 保持现有签名密钥、公钥和 Tauri `latest.json` 协议不变。
- Non-Goals:
  - 不镜像 macOS 更新包，不代理 GitHub 的全部 Release 资产。
  - 不引入 R2、付费存储、数据库、账号体系、自建服务器或新的更新签名体系。
  - 不让用户输入自定义更新源，也不为 v1.0.21 及更早的已发布二进制回写 endpoint。

## Decisions

### 固定双 endpoint 与平台分流

新客户端依次配置：

1. `https://logcrate-updates.pages.dev/latest.json`
2. `https://github.com/Strive-Sun/LogCrate/releases/latest/download/latest.json`

Pages 清单保留 GitHub 生成的版本、发布时间、说明、平台集合和签名，只把 Windows 平台的 NSIS `url` 改为 `https://logcrate-updates.pages.dev/releases/vX.Y.Z/<asset>`。macOS 平台 URL 保持 GitHub Release 地址。客户端继续只信任内置公钥，来源切换不改变可安装内容的身份。

### 确定性镜像暂存

新增 Node 脚本从最终 GitHub Release 资产构造一个独立暂存目录，而不是直接修改手工维护的站点目录。脚本必须：

- 校验 tag、清单版本、Windows 平台键、NSIS URL、非空签名和实际下载资产一致。
- 拒绝单个 Windows 镜像资产大于或等于 25 MiB，并在错误中报告实际大小。
- 按版本化路径复制 Windows NSIS 包，原样保留签名文本，仅改写 Windows URL。
- 生成 Pages `_headers`：`latest.json` 使用 `no-store`/重新验证语义，版本化包使用长期 `immutable` 缓存。
- 支持生成不含 `latest.json` 的 fallback-only 部署，以便安全切换发布状态。

脚本使用固定夹具覆盖成功、缺失资产、空签名、版本不一致、错误平台 URL、尺寸边界和 fallback-only 输出；测试不依赖网络或真实 secrets。

### 避免旧清单遮挡的发布顺序

Release matrix 先构建 Windows 与 macOS 产物并写入同一个 draft GitHub Release。汇总 job 在公开版本前验证完整 `latest.json`、Windows NSIS、macOS 更新包和签名配置，然后按以下顺序执行：

1. 将 fallback-only 站点部署到 Pages production，并确认公开 `latest.json` 不再返回有效清单。
2. 将已验证的 GitHub draft Release 公开；此时客户端访问 Pages 会转入 GitHub fallback。
3. 从同一批最终资产生成完整镜像，并以一次 Pages production deployment 同时上线 Windows 包与 `latest.json`。
4. 从公网重新获取 Pages 清单和 Windows 包，核对版本、URL、签名文本、长度或摘要以及缓存响应头。

如果第 1 步失败，不公开 GitHub Release；如果公开后镜像部署或验证失败，workflow 明确失败，且 Pages 保持 fallback-only 状态，不能自动恢复旧清单。恢复方式是修复后重新部署当前版本镜像；不得回滚到较旧的有效 `latest.json`。

### 凭据和权限

GitHub Actions 只通过 `CLOUDFLARE_API_TOKEN` 与 `CLOUDFLARE_ACCOUNT_ID` secrets 调用 Pages deployment API。token 只授予目标账户 Pages 编辑所需的最小权限，不写入仓库、构建产物或日志。项目名固定为 `logcrate-updates`，避免 workflow 输入任意部署目标。

## Risks / Trade-offs

- `pages.dev` 在部分中国大陆网络仍可能不可达 → 保留官方 GitHub endpoint 作为顺序 fallback，失败不阻断应用启动和手动重试。
- GitHub 公开与完整 Pages 部署无法跨服务实现真正事务 → 发布前先下线 Pages 清单，将中间态和失败态转换为明确的 GitHub fallback。
- macOS 仍依赖 GitHub 下载 → Pages 清单改善检查可达性，但不承诺改善 macOS 大包下载；避免踩到单文件限制导致不稳定发布。
- Pages 免费额度或平台限制未来可能变化 → 发布脚本在每次发布前检查包大小，部署失败会 fail loud，不静默生成部分镜像。
- v1.0.21 仍无 Pages endpoint → 发布说明需明确这是一次性的迁移限制。

## Migration Plan

1. 维护者在 GitHub 配置 Cloudflare account ID 与最小权限 API token secrets，不向 Agent 提供 secret 值。
2. 实现并验证镜像生成工具及 Release workflow，但不触发 tag 或远端发布。
3. 为新客户端加入 Pages 优先、GitHub 备用 endpoint，并完成 Windows/macOS 发布级构建与静态检查。
4. 下一个正式版本按新 workflow 发布；v1.0.21 用户仍从 GitHub 获取该迁移版本。
5. 发布后验证 Pages 与 GitHub 两条路径，再由维护者决定是否归档 change。

## Open Questions

- 无。Pages 项目名、公开域名、平台范围、缓存策略和失败回退顺序均已确定。
