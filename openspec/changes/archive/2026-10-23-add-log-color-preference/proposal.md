# Change: 增加日志正文与级别颜色自定义

## Why

日志正文按级别着色：含 INFO 的行显示为绿色，ERROR 红色，WARN 黄色，DEBUG/TRACE 灰色。大部分日志行都是 INFO，整屏绿色不利于长时间阅读。用户希望自行设置颜色，例如把正文改成黑色。

## What Changes

- 设置面板新增“日志颜色”分组，可以分别设置五项颜色：普通正文（不含级别的行）、ERROR、WARN、INFO、DEBUG/TRACE。
- 每项默认“跟随主题”，即保持现有颜色，并随深浅色主题切换；用户也可以用取色器选一个自定义颜色，再通过“恢复默认”回到跟随主题。
- 自定义颜色只影响日志正文文字，在深色和浅色主题下都生效。行号、查找高亮、字段匹配行背景、选区和界面其他部分保持不变。
- 选择立即生效并持久化，无效值逐项回退到跟随主题。
- 界面模板仍然不会改变日志级别色。

## Impact

- Affected specs: `application-settings`（新增日志颜色偏好）。
- Affected code: `src/util/`（新增颜色偏好模块）、`src/components/SettingsPanel.tsx`、`src/components/TopBar.tsx`、`src/App.tsx`、`src/util/logFont.ts` 或 `LogContent` 面板样式接线、`src/styles.css`、`src/i18n/messages.ts` 及对应前端测试。
- 不改变 Tauri 命令、事件、Rust 实现或依赖。
