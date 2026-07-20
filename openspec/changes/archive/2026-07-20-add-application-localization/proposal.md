# Change: 增加中英文应用界面

## Why

LogPeek 当前所有应用界面文案固定为简体中文，英文 README 和跨平台安装包无法为非中文用户提供一致体验。目录树、右键菜单、更新流程和托盘菜单分散在多个前端组件与 Rust 代码中，若只逐项替换文本，后续新增语言会持续遗漏和重复。

## What Changes

- 建立统一的应用本地化层，首批提供简体中文（`zh-CN`）和英文（`en`）两套完整消息字典。
- 默认跟随操作系统语言；系统语言为中文时使用简体中文，其它语言回退英文。
- 在设置面板增加语言选择，支持“跟随系统 / 简体中文 / English”，切换后立即生效并持久化。
- 翻译目录树、右键菜单、选项卡、日志状态、通知、设置、更新、确认弹窗、文件选择器、可访问性标签及应用自有错误提示。
- 语言切换同步更新 HTML `lang` 属性与系统托盘“显示主窗口 / 退出”菜单，不要求重启应用。
- 增加字典键一致性、系统语言解析、偏好持久化、插值及回退测试，防止新增界面文案遗漏翻译。

## Impact

- Affected specs: `application-interface`、`application-settings`
- Affected code: `src/App.tsx`、`src/components/*`、`src/api/*`、新增 `src/i18n/*`、`src-tauri/src/lib.rs`
- Dependencies: 使用项目内轻量字典与 React Context，不新增 npm 或 Cargo 依赖

