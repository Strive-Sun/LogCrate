## 1. Implementation

- [x] 1.1 实现确定性的 Pages 镜像暂存与 `latest.json` 校验/改写脚本，生成缓存头和 fallback-only 输出，并用离线夹具覆盖成功、签名、版本、资产匹配及 25 MiB 边界
- [x] 1.2 将脚本接入 Release workflow：双平台产物先进入 draft，验证 Cloudflare 配置与最终资产后依次部署 fallback-only、公开 GitHub Release、原子部署完整镜像并做公网验证；同步发布文档且确保失败状态可恢复
- [ ] 1.3 将 Tauri updater 改为 Pages 优先、GitHub 备用，更新发布/品牌静态检查，并完成 Windows 与 macOS 的 L3 更新配置、正式产物、签名清单及双路径 fallback 验证
