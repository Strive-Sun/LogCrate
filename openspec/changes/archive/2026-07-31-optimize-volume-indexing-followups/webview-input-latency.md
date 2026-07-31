# 真实 WebView 输入延迟采样说明

当前代码已在真实 `FileSearchPanel` 输入框接入采样：每次输入事件到下一帧记录一个原始样本，最多保留 200 个样本，并将最新报告放在：

```js
window.__logcrateSearchInputLatency
```

同时派发 `logcrate:search-input-latency` 事件，报告字段为 `sampleCount`、`samplesMs`、`p95Ms` 和索引阶段 `phase`。

## 尚缺证据

当前自动化环境没有真实 Tauri/WebView 驱动，不能合法地用 jsdom、后端查询耗时或合成 DOM 事件代替真实键入。因此 2.2 保持未完成。

## 手工验收步骤

1. 启动 Release Tauri 应用并打开文件搜索。
2. 在索引建立期间连续输入至少 30 个搜索词，再在查询 ready 后重复一轮。
3. 在 WebView DevTools 执行 `window.__logcrateSearchInputLatency`，保存 `samplesMs`、`p95Ms` 和当前索引阶段。
4. 分别完成三轮并归档原始报告；三轮 p95 均不超过 100ms 后，才可勾选 tasks.md 的 2.2。
