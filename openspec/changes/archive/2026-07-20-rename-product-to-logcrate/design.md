## Context

产品名称出现在 Tauri bundle 元数据、窗口标题、托盘、前端字典、README、技术设计、CI Release 标题和更新规范中。另有一批 `logpeek.*` 本地存储键、`com.logpeek.app` bundle identifier、缓存文件前缀、Rust crate/lib 名和 GitHub 仓库地址，它们虽然包含旧名称，但直接关系升级兼容或内部构建，并不都应同步替换。

当前图标为深色背景上的蓝色方框与斜线，缺少日志和归档语义；项目也没有可编辑的矢量母版，平台尺寸只能分别维护。

## Goals / Non-Goals

- Goals:
  - 所有用户可见位置统一显示 `LogCrate`。
  - 新图标在 README、窗口、任务栏、托盘、安装器和小尺寸文件列表中都能表达“归档日志”。
  - 已安装 LogPeek 的用户通过现有 updater 原位升级，监控目录、语言、更新和布局设置保持不变。
  - 建立单一 SVG 图标源，平台位图可重复生成。
- Non-Goals:
  - 不在本 change 中重命名 GitHub 仓库、组织或远程 URL。
  - 不清理历史 CHANGELOG、归档 OpenSpec 或旧 tag 中的 `LogPeek` 名称。
  - 不立即改动 Rust crate/lib 名、源码测试临时文件前缀或兼容性存储键。
  - 不在图标中加入 `L`、`LC` 或完整产品文字。

## Decisions

### 用户品牌与兼容标识分离

- 改为 `LogCrate`：Tauri `productName`、主窗口标题、前端品牌、托盘 tooltip/退出项、安装包显示名、Release 名、中英文字典、README 和当前技术文档。
- 保留 `com.logpeek.app`：Windows 升级身份、macOS bundle identity 和应用配置目录继续沿用，避免被系统识别为新应用。
- 保留 `logpeek.*` localStorage 键、`logpeek-cache`、现有 Watch 配置和 updater 公钥，避免迁移风险；代码注释说明其为 legacy compatibility key。
- Rust package/lib 名暂时保留，避免无用户价值的模块与产物路径重构；Tauri 对外 bundle 由 `productName` 决定。
- GitHub endpoint 暂时保留 `Strive-Sun/LogPeek`，旧安装和新安装继续使用同一签名 Release 源。

### 图标视觉语言

- 母版尺寸为 1024×1024 SVG，使用圆角方形深海军蓝底板，适合 Windows 与 macOS 统一识别。
- 主符号由两个部分组成：橙色的打开归档箱轮廓，箱内/上方三条青色水平日志行。
- 使用粗线、有限色彩和大负形；32px 下保留箱体与至少两条日志行，16px 下仍能区分为“箱体 + 内容”。
- 不使用渐变、细小文字、放大镜、纸张折角或过多阴影，避免小尺寸糊成一团。
- SVG 母版存入 `src-tauri/icons/logcrate.svg`；Tauri icon 工具生成 PNG、ICO 和 ICNS，禁止手工分别绘制平台版本。

建议配色：

| 角色 | 色值 | 用途 |
|---|---|---|
| 背景 | `#111827` | 稳定的深色圆角底板 |
| 归档箱 | `#F59E0B` | 暖色主符号，表达 crate/archive |
| 日志行 | `#22D3EE` | 高对比内容线，表达 text/log stream |
| 高光 | `#F8FAFC` | 仅在必要的小尺寸分隔处使用 |

### 生成与验收

- 使用 `npm run tauri icon src-tauri/icons/logcrate.svg` 或等价 Tauri CLI 从单一母版生成资产。
- 检查 1024、256、128、32 和 16px；PNG 必须有正确 alpha，ICO/ICNS 必须包含平台要求的尺寸。
- README 使用生成的 128px 图标；Windows 任务栏、托盘、安装器和 macOS bundle 至少各人工验证一次。
- 新图标先随未发布构建验证，用户确认后才归档和提交。

### 文档与历史边界

- README、README_ZH、当前 docs、OpenSpec project context 和主规范使用新品牌。
- CHANGELOG 在 `Unreleased` 新增明确的品牌迁移与兼容说明；旧版本章节保持当时的 `LogPeek` 原文。
- 已归档 OpenSpec、旧 release notes 和 tag 不批量改写，保留可审计历史。

## Risks / Trade-offs

- Windows 可能因 productName 改变安装目录显示 → 保持 identifier 并用 v1.0.6 安装后升级包做真实验证。
- macOS 名称改变但 bundle identifier 不变 → 验证覆盖安装、配置目录和 updater 重启。
- GitHub 仓库仍叫 LogPeek，品牌暂时不一致 → README 解释迁移，仓库改名单独取得授权后执行。
- 图标在 16px 丢失日志行 → 采用粗线与有限元素，并基于实际缩放图而非只看 SVG 大图验收。

## Migration Plan

1. 生成并人工确认新图标。
2. 修改用户可见品牌，保留 identifier、配置键、签名和 updater endpoint。
3. 从已安装的 LogPeek v1.0.6 执行新版本自动更新，确认只保留一个应用、配置与监控继续存在。
4. 发布时在 Release Notes 明确“LogPeek 已更名为 LogCrate”。
5. 若回滚，只恢复 productName、窗口/托盘文案和旧图标；兼容标识从未改变，无需数据回迁。
