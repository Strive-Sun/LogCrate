## 1. 品牌引用与兼容边界

- [x] 1.1 建立用户可见品牌引用清单，区分必须改名与必须保留的 legacy compatibility key
- [x] 1.2 将 Tauri productName、窗口标题、前端顶栏、中英文字典、托盘 tooltip/菜单和浏览器标题改为 LogCrate
- [x] 1.3 保留并注释 identifier、本地存储键、配置/缓存路径、签名公钥和旧 updater endpoint
- [x] 1.4 更新 Release workflow 显示名，不改变 tag、签名和 latest.json 生成协议

## 2. 图标系统

- [x] 2.1 创建 `logcrate.svg` 矢量母版，实现深色底板、橙色归档箱和青色日志行
- [x] 2.2 使用 Tauri icon 工具生成 PNG、ICO、ICNS 和平台附加尺寸
- [x] 2.3 自动检查生成文件、alpha、尺寸集合和配置引用，并查看 128px/32px 实际缩放效果
- [x] 2.4 在 Windows 窗口、任务栏和托盘中人工确认图标辨识度
- [ ] 2.5 在 Windows 安装器及 macOS bundle 中人工确认图标辨识度

## 3. 文档与规格

- [x] 3.1 更新英文/中文 README 的名称、说明、Logo alt 与当前品牌文字，保留可用的旧仓库链接
- [x] 3.2 更新技术设计、开发流程、OpenSpec project context 和当前主规范中的用户品牌
- [x] 3.3 在 CHANGELOG Unreleased 逐条记录改名、新图标和升级兼容策略，历史版本文本保持不变

## 4. 升级与验证

- [x] 4.1 验证现有配置、语言、更新和布局存储在改名后继续读取
- [ ] 4.2 从已安装 LogPeek v1.0.6 升级到 LogCrate 测试构建，确认不产生并行安装或配置丢失
- [x] 4.3 运行前端格式/测试/lint/构建、Rust fmt/clippy/测试、Tauri build 检查和 OpenSpec 严格校验
- [x] 4.4 扫描非历史文件中的 LogPeek 引用，确认剩余项均属于明确保留范围
