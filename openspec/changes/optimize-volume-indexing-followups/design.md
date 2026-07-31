## Context

当前 `search.rs` 仍同时承担 manager、协调、provider、SQLite、恢复、查询索引切换和状态报告。现有 active/staging/previous、重试和 Tantivy merge 等待已经存在，因此本变更是补齐边界、观测和覆盖，不是从零重写切换流程。

## Goals / Non-Goals

- Goals：建立最小可用的结构化 operation telemetry；抽离 query store；以 `scope_key` 定义单范围索引替换边界；重测当前 HEAD 的单范围恢复和 WebView p95；补充 Windows 剩余可靠性证据。
- Non-Goals：不预设 provider 必须低于五分钟；不把旧版本基线当作当前验收样本；不改变搜索结果、权限或 UI 非阻塞语义。

## Decisions

- 执行顺序为：最小 telemetry → query store → 当前 HEAD 基线 → 按收益决定其他边界拆分 → 单范围原子替换 → Windows 诊断与真机覆盖 → 条件化性能优化。避免在没有新证据前进行大规模重构。
- `IndexOperation` 使用稳定 operation ID，记录 generation、阶段时间、发现数、可搜索数、查询结果数、错误和终态；逐 scope 记录 provider、阶段、计数和耗时。性能报告只从该快照生成，零散日志仅用于诊断。
- 基线固定设备、C:/D: 范围、Release 构建、数据快照和 operation 起止点。当前 HEAD 先执行三轮；每个实际优化以前后同夹具三轮比较。历史数据只作为背景。
- 查询索引范围键命名为规范化 exact `scope_key`，覆盖整卷和目录根，并定义大小写、路径分隔符及跨平台规则。`volume_key` 仅作为 NTFS 整卷 provider 的实现别名。
- 单范围替换期间继续读取旧 active；新范围 staging 完成并核对计数和代表性查询后原子发布。失败保留旧 active 和 SQLite 可用；共享 staging 必须保留未受影响范围。
- Windows 日志补充 operation ID、阶段、源/目标路径、目录状态、并发查询、句柄诊断和重试次数；不重复实现已有 retry、merge wait 和 rollback。
- 只有结构化基线确认真实瓶颈时才进行生命周期性能优化；若门槛已满足，则记录无需优化并关闭该条件任务。

## Risks / Trade-offs

- 按 scope 删除和 schema 迁移会增加索引元数据与临时磁盘占用，必须先验证可回滚性并记录峰值磁盘。
- 更详细遥测会增加少量日志量；默认不记录文件内容，只保留聚合指标和可定位错误上下文。

## Open Questions

- 共享 staging、独立物理索引或其他布局哪一种能在可接受成本下满足实测恢复目标？
- Windows 句柄占用的主要来源是什么？需由多轮真机诊断确认。
