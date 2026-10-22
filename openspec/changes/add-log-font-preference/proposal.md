# Change: 增加日志正文字体与字号偏好

## Why

日志正文固定为 12px 等宽字体、18px 行高，长时间阅读时偏小且容易疲劳。用户希望像 Windows 记事本那样，选择更舒适的字体和更大的字号。

## What Changes

- 设置面板新增“日志字体”和“日志字号”两个下拉选项，选择后立即生效并持久化；缺失或无效值回退到当前默认（默认等宽、12px）。
- 字体提供预设：默认等宽（现有字体栈）、Consolas、Lucida Console、微软雅黑（与记事本观感相近的比例字体）；每个预设都带 macOS 回退字体。
- 字号提供 12、13、14、15、16、18、20px 档位，行高随字号按约 1.5 倍自动计算，行号槽宽随字号缩放。
- 作用范围仅限日志正文、行号与字段筛选栏；目录树、选项卡、设置、AI 工作区和 DOCX 预览不变。
- 日志虚拟列表的行高由固定 18px 改为跟随字号；切换字体或字号、重新加载、查找定位与字段筛选定位都按当前行高计算，切换后保持当前阅读的顶部行。
- 选择比例字体时，字段筛选栏的等宽列宽与拖动参考线按字符宽度近似计算，可能与正文文字略有偏差；字段解析边界不受影响。

## Impact

- Affected specs: `application-settings`（新增日志字体与字号偏好）。
- Affected code: `src/components/LogContent.tsx`、`src/components/LogRow.tsx`、`src/components/SettingsPanel.tsx`、`src/components/TopBar.tsx`、`src/App.tsx`、`src/util/`（新增偏好模块）、`src/styles.css`、`src/i18n/messages.ts` 及对应前端测试。
- 不改变 Tauri 命令、事件、Rust 实现或依赖。
