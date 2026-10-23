## Context

- 日志正文 `.log-view` 是虚拟列表的滚动容器，使用原生滚动条；上一提交 `05d243d` 将其纵向滚动条设为 18px 宽、滑块最小 48px，并保留 `--scrollbar-thumb-color` 驱动的自动渐隐。
- 原生滚动条点击轨道的行为是逐页翻动，网页无法配置。
- 验证（2026-10-23，本机 Microsoft Edge Chromium 无头模式，CDP 注入真实鼠标事件）：点击轨道时滚动容器能收到 `mousedown`，且 `offsetX >= clientWidth` 可识别点击落在滚动条区域；但在 `mousedown` 上调用 `preventDefault()` 后，原生仍然翻页（两次试验 scrollTop 分别为 263 与 262，均约一页）。因此无法在保留原生滚动条的前提下改写轨道点击行为。

## Goals / Non-Goals

- Goals：点击轨道直接跳转并可继续拖动；滑块拖动、滚轮、键盘滚动保持现状；外观与渐隐和现状一致；布局宽度不变。
- Non-Goals：横向滚动条的点击跳转；目录树等其它区域的滚动条；滚动条的键盘焦点与读屏交互（原生滚动条同样不可聚焦，正文键盘滚动不受影响）；突破浏览器最大元素高度的超大日志映射。

## Decisions

- 隐藏 `.log-view` 的原生纵向滚动条（`::-webkit-scrollbar` 宽度设为 0，横向高度仍为 18px），在其右侧放置 18px 宽的自绘轨道列 `.log-vscroll`。正文与轨道包在一个横向 flex 容器中，轨道占据原来原生纵向滚动条的宽度，正文可见宽度与现状一致。轨道底部留出横向滚动条的高度，对齐正文的可见区域。
- 几何计算放在纯函数模块 `scrollbarGeometry.ts`：
  - 滑块长度 = `max(48, 轨道长度 × clientHeight / scrollHeight)`，不超过轨道长度；内容不足一屏时隐藏滑块。
  - 滑块位置 = `scrollTop / (scrollHeight − clientHeight) × (轨道长度 − 滑块长度)`。
  - 由滑块位置反算 scrollTop，并夹在 `[0, scrollHeight − clientHeight]` 内。
- 交互使用 Pointer Events 与 `setPointerCapture`：
  - 在轨道（非滑块）按下：目标滑块位置 = 指针位置 − 滑块长度 / 2，立即设置 `scrollTop`，随后按“抓住滑块中心”继续跟随指针移动直至松开。
  - 在滑块按下：记录按下点相对滑块顶部的偏移，拖动时保持偏移不变。
  - 仅响应主键；滚轮在轨道上时把 `deltaY` 转发给正文。
- 滑块的显示通过兄弟选择器跟随 `.log-view.is-scrolling`，沿用现有 `--scrollbar-thumb-visible/-hover` 颜色变量和 450ms 渐隐；拖动期间持续产生滚动事件，因此保持可见。`prefers-reduced-motion` 下取消过渡。
- 滑块几何在正文 `scroll`、尺寸变化（ResizeObserver）以及行数或行高变化后，通过 `requestAnimationFrame` 合并更新，直接写入滑块元素样式，不触发 React 重渲染。

## Risks / Trade-offs

- 自绘滚动条与系统原生外观可能存在细微差异 → 尺寸、圆角与颜色沿用现有样式变量。
- 隐藏原生纵向滚动条后，依赖原生滚动条的辅助功能（如屏幕阅读器）不会提供滚动条控件 → 正文仍可用键盘与读屏滚动，原生滚动条本身也不可聚焦。
- 若未来 WebView 改变 `::-webkit-scrollbar` 的宽度处理，原生纵向滚动条可能重新出现 → 测试锁定样式规则，真机验收确认。

## Migration Plan

纯前端交互变化，无持久化数据；回滚只需恢复原生纵向滚动条并移除自绘轨道。
