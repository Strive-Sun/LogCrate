# Change: 优化多卷索引后续验证与恢复

## Why

并行索引、可搜索快照发布和启动初始化隔离已经落地，但搜索实现仍把协调、持久化、查询索引切换、恢复和遥测集中在 `search.rs` 中。当前缺少统一 operation ID、逐范围生命周期快照和真实 Tauri/WebView 输入延迟证据；单个搜索范围的索引替换也尚未形成完整的原子发布与回滚边界。

历史基线曾出现单卷完整可搜索略超 60 秒，但数据来自较早版本；当前 provider 总耗时也已达到现行五分钟门槛。因此本变更先建立当前 HEAD 的可比基线，再决定是否需要性能优化，不预设必须进行五分钟生命周期优化。

## Implementation Strategy

本变更分批实施，每批独立验证、记录证据后再进入下一批：

1. 可观测性：建立 operation/scope telemetry，并取得当前 HEAD 基线。
2. 索引正确性：引入 `scope_key`，实现单范围删除、重建、原子发布和回滚。
3. 可靠性验证：补充 Windows 切换、启动恢复和失败后的查询/UI 真机覆盖。
4. 条件性性能优化：仅在前述批次的基线确认瓶颈后启动，不作为默认交付内容。

## What Changes

- 以维护收益为依据逐步拆分 `search.rs` 的 query store、persistence、recovery、coordinator 和 telemetry 边界，保持现有 Tauri 命令、事件和搜索结果语义不变。
- 建立统一的结构化 `IndexOperation`/scope 生命周期快照，以稳定 operation ID 关联调度、provider、查询可用、持久化、事件交接和最终状态。
- 使用当前 HEAD、固定设备和数据快照建立三轮应用级基线，并采集真实 Tauri/WebView 输入 p95。
- 以规范化 exact `scope_key`（整卷或目录根）作为查询索引范围身份；实现单范围删除、重建、active/staging/previous 原子发布和失败回滚。只有 NTFS 整卷 provider 才可将该字段具体命名为 `volume_key`。
- 补充 Windows 索引切换的剩余诊断、外部占用和多轮真机覆盖；不重复实现已经存在的重试、merge 等待和回滚机制。
- 仅当新结构化基线确认存在回归或新瓶颈时，才实施 provider 生命周期优化。

## Impact

- Affected specs: `file-search`（修改现有多卷性能、恢复和生命周期要求，避免重复 requirement）
- Affected code: 搜索协调与查询索引存储、恢复/持久化边界、结构化遥测、Windows 切换诊断和真实 UI 基准工具
- 不改变搜索结果、权限边界、Tauri 载荷或后台非阻塞语义。
