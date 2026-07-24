# application-branding Specification

## Purpose
TBD - created by archiving change rename-product-to-logcrate. Update Purpose after archive.
## Requirements
### Requirement: LogCrate 产品身份

系统 SHALL 在当前用户可见的应用元数据、主窗口、顶栏、托盘、安装包、正式 Release 和文档中使用产品名 `LogCrate`，并以归档日志阅读器作为用途说明。历史版本记录 MAY 保留发布时使用的旧名称 `LogPeek`。

#### Scenario: 查看当前产品名称

- **WHEN** 用户启动应用、查看窗口/托盘、安装包、当前 README 或正式 Release
- **THEN** 系统显示 `LogCrate`，且主要文档说明其用于监控目录和免手动解压阅读归档日志

#### Scenario: 保留历史记录

- **WHEN** 用户查看旧版本 CHANGELOG、归档规格或旧 tag
- **THEN** 系统允许这些不可变历史内容继续使用当时的 `LogPeek` 名称

### Requirement: 可辨识的归档日志图标

系统 SHALL 使用同一矢量母版生成平台图标，图形 MUST 在常用尺寸下同时表达归档容器和文本日志，不依赖文字识别。Windows、macOS、README、任务栏、托盘和安装器 SHALL 使用同一品牌图形。

#### Scenario: 查看正常尺寸图标

- **WHEN** 用户在 README、应用列表或安装器中查看 128px 及以上图标
- **THEN** 图标清晰显示打开的归档箱和日志行，颜色与边界完整

#### Scenario: 查看小尺寸图标

- **WHEN** 系统在任务栏、托盘或文件列表以 16px–32px 显示图标
- **THEN** 图标仍能辨认出箱体和内容线，不出现无法区分的细碎文字或模糊元素

### Requirement: 开发阶段品牌标识统一

系统 SHALL 在首次正式发布前统一使用 LogCrate 品牌标识。Tauri bundle identifier MUST 为 `com.logcrate.app`，Cargo package、Rust library、npm package、缓存目录、本地存储键、内部 ID 及运行时临时文件前缀 MUST 使用 `logcrate`，且新构建 MUST NOT 为未发布的 LogPeek 开发版本保留配置迁移逻辑。

#### Scenario: 编译应用

- **WHEN** 开发者编译桌面应用或索引服务
- **THEN** Cargo 和 npm 构建输出使用 `logcrate`，不再显示 `logpeek`

#### Scenario: 首次启动开发版本

- **WHEN** 用户启动尚未正式发布的 LogCrate 构建
- **THEN** 应用使用 `com.logcrate.app`、`logcrate-cache` 和 `logcrate` 本地存储键创建全新的开发期数据
