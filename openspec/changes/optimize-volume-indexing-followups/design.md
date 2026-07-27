## Context

`parallelize-volume-indexing` 已完成并行调度和可搜索快照发布。真机 UI 测试显示 C provider 585.5 秒、D provider 307.3 秒，最终有 4,895,619 个可搜索文件且查询约 16ms 返回结果。此前自动化持久化指标不能代表完整应用生命周期，因此需要单独测量并优化。

## Goals / Non-Goals

- Goals: 降低真实 provider 生命周期；建立应用级基线；测量 WebView 输入 p95；改善单卷启动恢复；验证并记录 Windows 索引目录切换失败。
- Non-Goals: 不把当前五分钟目标或旧的 cargo PATH 环境问题当作已解决功能；不牺牲结果正确性、权限边界或后台非阻塞行为。

## Decisions

- 所有性能报告必须拆分调度、MFT/USN、路径解析、查询索引可用、持久化和事件交接耗时，并同时记录发现数、可搜索数和查询结果数。
- 基线使用同一设备、相同 C:/D: 范围、Release 构建和固定数据快照；修改前基线只能通过重新构建旧版本或归档的可执行物取得，不能用串行单元测试代替。
- WebView p95 只接受真实 Tauri/WebView 交互采样；后端查询延迟或 jsdom 测试不得作为替代。
- 单卷恢复优先评估 volume 字段、按卷删除或按卷 segment；在 schema 迁移前保持旧索引可回滚。
- Windows 切换日志必须包含阶段、源路径、目标路径、活动索引、并发查询、`.next`/`.previous` 状态和重试次数；失败时保留旧活动索引可查询。

## Risks / Trade-offs

- 按卷删除或 segment 可能增加索引元数据和迁移复杂度；先用实验性 schema 与可回滚迁移验证。
- 更详细遥测会增加少量日志量；默认仅保留聚合指标和可定位的错误上下文，不记录文件内容。

## Open Questions

- Tantivy 采用 volume 字段过滤、按卷 segment，还是两者组合，才能在可接受的写入开销下满足 60 秒恢复目标？
- Windows 文件句柄占用来自 Tantivy reader、并发查询、其他进程还是杀毒软件？需要通过诊断日志和多轮真机复现确认。
- provider 总耗时超过五分钟时，主要瓶颈是 MFT、路径解析、合并还是持久化？需要阶段数据驱动优化顺序。
