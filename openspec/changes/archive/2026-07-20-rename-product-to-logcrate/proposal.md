# Change: 将产品品牌迁移为 LogCrate

## Why

`LogPeek` 只表达“查看日志”，无法让首次接触的用户理解目录监控、归档日志和免手动解压阅读这一核心用途。候选名 `Logwatch` 与已有 Unix 日志分析产品冲突；`LogCrate` 在基础重名检查中辨识度更高，并以“装载日志的归档箱”表达产品定位。

现有图标只是抽象的方框与斜线，在任务栏、托盘和 README 中无法快速传达日志或归档含义，需要随品牌一起重构。

## What Changes

- 将用户可见产品名从 `LogPeek` 统一改为 `LogCrate`，副标题采用 “Archive Log Viewer / 归档日志阅读器”。
- 更新窗口标题、应用顶栏、托盘、安装包显示名、Release 标题、中英文字典、README、技术文档和当前主规范。
- 新增可编辑 SVG 图标母版：深色圆角底板、橙色打开归档箱和青色日志行，不包含文字，在小尺寸下仍可识别。
- 由 SVG 母版统一生成 Windows ICO、macOS ICNS 与 Tauri 所需 PNG 尺寸，并验证透明度、缩放和任务栏/托盘效果。
- 保留 `com.logpeek.app`、现有本地存储键、配置/缓存路径、updater 公钥和旧 GitHub Release endpoint，确保现有安装可原位升级且用户配置不丢失。
- GitHub 仓库改名不在本 change 中自动执行；若后续显式改名，再依赖 GitHub 重定向验证并更新远程链接。

## Impact

- Affected specs: `application-branding`（新增）、`application-lifecycle`、`application-updating`
- Affected code/assets: `src-tauri/tauri.conf.json`、`src-tauri/icons/*`、`src-tauri/src/lib.rs`、`src/i18n/messages.ts`、`src/components/TopBar.tsx`、Release workflow、README 与 docs
- Compatibility: 必须保留 bundle identifier、配置键和签名信任链；下一正式版本需验证从 LogPeek v1.0.6 自动更新到 LogCrate
- Dependencies: 使用 SVG 母版和现有 Tauri icon 工具，不新增运行时依赖
