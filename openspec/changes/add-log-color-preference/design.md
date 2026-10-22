## Context

- 级别由 `detectLevel` 识别（ERROR/WARN/INFO/DEBUG/TRACE/FATAL），`LogRow` 给文字加 `lvl-<LEVEL>` 类；CSS 中 `.lvl-ERROR/WARN/INFO/DEBUG/TRACE` 分别使用主题变量 `--error/--warn/--info/--debug`，深浅色主题各有一套取值。FATAL 当前没有颜色规则，显示为普通正文色。
- 普通正文颜色继承主题 `--fg`。
- 上一变更已经在 `.log-content-panel` 上写入字体相关 CSS 变量（`logFontStyle`），日志面板样式只在该容器生效。

## Goals / Non-Goals

- Goals：五项颜色可独立自定义或跟随主题；默认外观与现状一致；只影响日志正文文字。
- Non-Goals：为 FATAL 新增独立颜色（保持现状）；按深浅色主题分别保存两套自定义颜色；自定义行号、背景、查找高亮或字段匹配高亮颜色；颜色对比度自动校正。

## Decisions

- 偏好以 `logcrate.logColors.v1` 保存为对象，键为 `body | error | warn | info | debug`，值为 `#rrggbb` 小写字符串；缺省的键表示跟随主题。读取时逐项校验，不合法的键视为跟随主题；存储不可用时保留当前进程内选择。
- 自定义颜色通过 `.log-content-panel` 上的 CSS 变量 `--log-color-body/error/warn/info/debug` 注入，只有自定义了的项才写变量。CSS 规则改为 `color: var(--log-color-error, var(--error))` 等形式，普通正文 `.log-line .txt` 使用 `var(--log-color-body, var(--fg))`；未设置时回退到主题变量，外观与现状完全一致。
- DEBUG 与 TRACE 沿用现有合并规则，共用一项颜色。
- 设置面板以原生 `<input type="color">` 选色，每项提供可访问名称，并配一个“恢复默认”按钮；跟随主题时取色器显示当前主题的实际颜色作为参考，但在用户主动选色之前不保存。
- 偏好状态由 App 持有，与字体偏好合并成同一份面板样式对象，传给全部 `LogContent`。

## Risks / Trade-offs

- 同一自定义颜色在另一种主题下可能对比度不足 → 设置说明中提示，用户可随时恢复默认；不做自动校正。
- 原生取色器在不同 WebView 中外观不同 → 只依赖 `input type=color` 的标准值格式 `#rrggbb`。

## Migration Plan

缺少偏好即全部跟随主题，现有用户视觉不变；回滚只需移除偏好读取，存储键残留无害。
