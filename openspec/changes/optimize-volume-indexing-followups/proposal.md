# Change: 优化多卷索引后续验证与恢复

## Why

当前并行索引已经能够完成索引、报告完整耗时并返回搜索结果，但真实应用中的 provider 生命周期仍可能超过五分钟。原变更还缺少修改前应用级基线和真实 WebView 输入延迟数据；Windows 索引切换与单卷启动恢复也需要生产级验证。

## What Changes

- 按协调、存储、查询、恢复和遥测职责拆分 `search.rs`，保持现有 Tauri 命令、事件和搜索结果语义不变。
- 建立统一、结构化的索引 operation/volume 生命周期模型，以同一 operation ID 关联调度、逐卷 provider、查询可用、持久化、事件交接和最终状态收敛指标。
- 使用同一指标定义和固定数据快照，重建修改前版本与当前版本的完整应用基线，并采集真实 WebView 输入响应 p95。
- 为 Tantivy 文档增加稳定的 `volume_key`，以按卷删除和重建作为正确性机制；segment 仅作为可选性能优化，不承担卷身份语义。
- 通过可回滚的 schema/index 迁移和按卷原子替换改善单卷启动恢复，并以 60 秒内完整可搜索为目标。
- 为 Windows active/staging/previous 切换增加生产诊断、遥测和 `os error 5` 复现验证。
- 最后以真实应用/UI 指标定位瓶颈，优化超过五分钟的 provider 生命周期；未经前序架构、观测、恢复和可靠性任务自测通过，不进入该阶段。

## Impact

- Affected specs: `file-search`
- Affected code: 索引协调器、生命周期遥测、Tantivy schema/按卷替换、SQLite 恢复、Windows 索引切换、UI 性能遥测与基准工具
- 不改变当前已验证的搜索结果和状态语义；五分钟目标是后续优化目标，不宣称本变更已达标。
