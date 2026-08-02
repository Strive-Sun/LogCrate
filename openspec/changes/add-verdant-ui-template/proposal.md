# Change: 增加绿色“森绿”界面模板

## Why

现有“原生”“极光”“琥珀”分别覆盖紧凑原生、冷色发光和暖色卡片风格，但缺少以绿色为主的自然、沉稳视觉选择。用户希望在不改变日志阅读布局和交互语义的前提下增加一套绿色界面模板。

## What Changes

- 在设置面板的界面模板区域增加“森绿”（`verdant`）选项，提供中英文名称、说明、缩略预览和明确选中状态。
- “森绿”使用森林绿与薄荷绿强调色、柔和自然渐变、清晰边界和克制阴影，并同时适配深色与浅色配色。
- 模板切换立即生效并沿用现有前端偏好持久化、重启恢复、键盘导航、窄窗口和减少动态效果行为。
- 将现有丰富模板共享样式覆盖扩展到“森绿”，但不改变日志正文排版、字段宽度、虚拟列表测量、功能布局及既有状态色语义。

## Impact

- Affected specs: `application-settings`
- Affected code: `src/util/uiTemplate.ts`、`src/components/SettingsPanel.tsx`、`src/i18n/messages.ts`、`src/styles.css` 及相关前端测试
- 不修改 Rust、Tauri IPC、持久化 schema、日志解析、搜索索引、安装器或发布流程
