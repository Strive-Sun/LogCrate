## Context

- 正文字体由 CSS 变量 `--log-font`（Cascadia Code/Consolas/SF Mono/Menlo/monospace）和 `.log-content-panel` 上的 `--log-font-size: 12px` 控制；`.log-line` 与字段栏写死 `line-height: 18px`，行号槽宽 `--log-gutter-width: 72px`。
- `LogContent` 中虚拟列表 `estimateSize`、`LogRow` 高度、滚动定位（查找、字段锚点、清除筛选恢复、重载快照）均硬编码 18px。
- 字段筛选栏的统一列宽和拖动参考线使用 `ch` 单位；按现行 spec，字段栏视觉等宽，不要求与正文字符列对齐，字段解析边界与视觉宽度无关。

## Goals / Non-Goals

- Goals：可选预设字体与字号档位；行高随字号变化且所有滚动定位一致；切换时保持阅读位置；默认值与现状完全一致。
- Non-Goals：自动换行；Ctrl+滚轮缩放；自定义任意字体名；调整目录树、选项卡等界面字号；比例字体下字段栏与正文的像素级对齐。

## Decisions

- 偏好以 `logcrate.logFont.v1` 保存为 `{ family, size }`，`family` 取值 `default | consolas | lucida | yahei`，`size` 取值 `12 | 13 | 14 | 15 | 16 | 18 | 20`；任一字段无效时该字段回退默认，存储不可用时保留当前进程内选择，与界面模板偏好一致。
- 行高 `rowHeight = round(size * 1.5)`，12px 时为 18px，与现状一致。行号槽宽按 `size * 6` 取整（12px 时 72px）。
- App 将字体栈、字号、行高、槽宽写成 `.log-content-panel` 上的 CSS 变量，并把 `rowHeight` 作为 prop 传给 `LogContent`；`LogContent` 用它替换全部 18 常量，`LogRow` 接收行高。
- 字号或字体变化时，`LogContent` 在重新测量前按旧行高记录顶部可见行与行内偏移比例，测量后按新行高恢复，复用重载快照的定位方式。
- 预设字体栈：
  - 默认等宽：沿用现有栈。
  - Consolas：`'Consolas', 'Menlo', 'Microsoft YaHei', monospace`。
  - Lucida Console：`'Lucida Console', 'Monaco', 'Microsoft YaHei', monospace`。
  - 微软雅黑：`'Microsoft YaHei UI', 'Microsoft YaHei', 'PingFang SC', 'Segoe UI', sans-serif`。
- 界面模板继续不改变正文字体指标；字体偏好是独立设置，两者可自由组合。

## Risks / Trade-offs

- 比例字体下 `ch` 只代表“0”的宽度，字段栏列宽和拖动参考线与正文文字会有偏差 → 设置说明中提示，字段解析不受影响。
- 字号变大时可视行数减少，虚拟列表单页读取量不变 → 不影响正确性。
- 用户机器缺少所选字体时浏览器按字体栈回退 → 预设均包含跨平台回退。

## Migration Plan

缺少偏好即默认等宽 12px，现有用户视觉不变；回滚只需移除偏好读取，存储键残留无害。
