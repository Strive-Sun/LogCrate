# Change: 增加 Cloudflare Pages 更新镜像

## Why

LogCrate 当前只从 GitHub Releases 检查并下载更新，中国大陆用户经常因 GitHub 连接失败而无法完成更新。现有 Cloudflare Pages 项目 `logcrate-updates` 已可访问，适合在不改变 updater 签名信任的前提下承载体积较小的 Windows 更新包和更新清单。

## What Changes

- 将新客户端的 updater endpoint 固定为 Cloudflare Pages 镜像优先、官方 GitHub Releases 备用，并继续使用现有 updater 公钥验签。
- 在 Pages 上发布无长期缓存的 `latest.json`，将 Windows NSIS 下载地址改写为同一 Pages 部署中的版本化地址；macOS 更新包继续从 GitHub 下载。
- 增加确定性的镜像暂存、清单校验和 URL 改写工具，拒绝缺失签名、版本不一致、非 NSIS Windows 包或达到 Pages 25 MiB 单文件限制的产物。
- 调整 Release workflow：先生成完整 draft Release，发布前让 Pages 清单暂时不可用，再公开 GitHub Release，最后原子部署并验证完整 Pages 镜像；镜像失败时明确失败并保持 GitHub fallback 可用。
- 记录 Cloudflare Pages 项目、GitHub Actions secrets 和发布恢复步骤；不在仓库、日志或应用包中保存 Cloudflare token。

## Impact

- Affected specs: `application-updating`
- Affected code: Tauri updater 配置、Release workflow、发布检查、镜像生成脚本与测试、发布文档、`logcrate-updates` Pages 站点内容
- External configuration: Cloudflare Pages 项目 `logcrate-updates`，GitHub Actions secrets `CLOUDFLARE_API_TOKEN` 与 `CLOUDFLARE_ACCOUNT_ID`
- Compatibility: 已发布的 v1.0.21 及更早版本仍只访问 GitHub；用户需要先通过 GitHub 升级到包含双 endpoint 的版本，后续版本才能优先使用 Pages
- Security: updater 公钥、签名私钥、签名格式与 bundle identifier 均不改变；Pages 只承担分发，不成为新的签名信任根
