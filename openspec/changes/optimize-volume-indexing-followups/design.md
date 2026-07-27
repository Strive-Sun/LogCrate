## Context

`parallelize-volume-indexing` 已完成并行调度和可搜索快照发布。真机 UI 测试显示 C provider 585.5 秒、D provider 307.3 秒，最终有 4,895,619 个可搜索文件且查询约 16ms 返回结果。此前自动化持久化指标不能代表完整应用生命周期，因此需要单独测量并优化。

## Goals / Non-Goals

- Goals: 先明确搜索模块边界；建立结构化观测与应用级基线；测量 WebView 输入 p95；改善单卷启动恢复；验证并记录 Windows 索引目录切换失败；最后降低真实 provider 生命周期。
- Non-Goals: 不把当前五分钟目标或旧的 cargo PATH 环境问题当作已解决功能；不牺牲结果正确性、权限边界或后台非阻塞行为。

## Decisions

- 变更严格按照架构拆分、结构化观测、基线验证、单卷恢复、Windows 可靠性、生命周期优化的顺序执行。每个编号任务只有在实现完整、其自测标准全部通过并留下结果证据后才能标记完成和开始下一任务；失败、跳过或尚需人工验证均视为未完成。
- 将现有搜索实现按职责拆分为 manager、coordinator、provider、persistence、query store、recovery 和 telemetry 边界。拆分保持单一协调器/单 writer、generation 取消、Tauri 命令与事件载荷兼容，不借重构改变搜索结果或权限语义。
- 引入结构化 `IndexOperation` 生命周期快照。每次启动恢复、重建或修复操作使用稳定的 operation ID，并记录 generation、开始时间、查询可用时间、持久化完成时间、事件交接完成时间、状态收敛时间和最终结果核对；每个 volume 子记录使用相同 operation ID 关联 provider、策略、阶段、计数、耗时和错误。
- 所有性能报告必须从该结构化快照生成，拆分调度、MFT/USN、路径解析、查询索引可用、持久化和事件交接耗时，并同时记录发现数、可搜索数和查询结果数。零散日志可用于诊断，但不得作为指标定义的唯一来源。
- 基线使用同一设备、相同 C:/D: 范围、Release 构建、固定数据快照和完全相同的 `IndexOperation` 起止点；修改前基线只能通过重新构建旧版本或归档的可执行物取得，不能用串行单元测试、单卷快速阶段或 MFT 子阶段代替完整应用基线。
- WebView p95 只接受真实 Tauri/WebView 交互采样；后端查询延迟或 jsdom 测试不得作为替代。
- Tantivy schema 增加稳定、规范化且可精确匹配的 `volume_key` 字段，`SearchIndexEntry` 必须携带该字段。单卷恢复通过 `delete_term(volume_key)` 后写入该卷新文档实现；Tantivy segment 可能被自动合并，因此 segment 只允许作为性能优化，不作为卷身份或正确性边界。
- 单卷替换沿用 active/staging/previous 发布模型：替换期间查询继续读取旧 active 快照，新的卷快照完成并校验计数后再原子发布；发布失败保留旧 active 可查询。若采用共享 staging，必须从旧 active 复制或重建未受影响卷，不能让单卷操作丢失其它卷结果。
- `volume_key` 引入时提升查询索引 schema 版本。迁移先构建新 staging 索引并核对逐卷计数和代表性查询，再切换 active；旧索引保留为 previous 直到新索引确认可用，失败时无需修改或清空 SQLite 即可回滚。
- Windows 切换日志必须包含阶段、源路径、目标路径、活动索引、并发查询、`.next`/`.previous` 状态和重试次数；失败时保留旧活动索引可查询。
- 生命周期性能优化必须放在最后，且只能依据前序结构化基线选择瓶颈；不得为追求单一阶段耗时牺牲结果正确性、未受影响卷持续查询、UI 响应、权限边界或崩溃回滚。

## Risks / Trade-offs

- 按卷删除或 segment 可能增加索引元数据和迁移复杂度；先用实验性 schema 与可回滚迁移验证。
- 从旧 active 构建共享 staging 可能增加临时磁盘占用和单卷恢复复制成本；基准必须同时记录总耗时、峰值磁盘和未受影响卷持续可查询时间，再决定是否引入更细粒度的物理索引布局。
- 更详细遥测会增加少量日志量；默认仅保留聚合指标和可定位的错误上下文，不记录文件内容。

## Open Questions

- 在 `volume_key` 正确性边界确定后，共享索引复制、按卷独立物理索引或辅助 segment 策略中，哪一种能在可接受的磁盘与写入开销下满足 60 秒恢复目标？
- Windows 文件句柄占用来自 Tantivy reader、并发查询、其他进程还是杀毒软件？需要通过诊断日志和多轮真机复现确认。
- provider 总耗时超过五分钟时，主要瓶颈是 MFT、路径解析、合并还是持久化？需要阶段数据驱动优化顺序。
