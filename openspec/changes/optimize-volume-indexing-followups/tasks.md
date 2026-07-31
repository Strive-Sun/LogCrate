> 执行本变更前必须按 `规则手册/说明.md` 完整执行 Step-0 至 Step-5。

## Phase 1：可观测性

- [x] 1.1 定义 `IndexOperation` 和逐 scope 生命周期快照，统一 operation ID、阶段、计数、错误和终态。
  - **自测标准**：成功、取消、单 scope 失败和持久化中断均能生成字段完整且无负耗时/终态漂移的报告。
- [x] 1.2 将 active/staging/previous 查询索引生命周期抽取到 query store，保持现有行为和载荷兼容。该项可在 1.1 和基线完成后实施。
  - **自测标准**：打开、部分查询、完整切换、遗留目录恢复和失败回滚测试通过。
- [x] 1.3 根据基线和维护收益决定是否继续拆 persistence、recovery、coordinator；不以“全部拆完”作为前置条件。
  - **自测标准**：每个实际拆分均有边界测试、Rust/前端测试、格式和 Clippy 证据。
  - **决策记录**：当前 change 暂不继续拆分 persistence、recovery、coordinator；三轮基线已定位持久化尾部，但本批目标是可观测性、索引正确性和可靠性验证，继续拆分会扩大回归面。后续如需优化持久化，另建 change 并以本次 operation 快照作为基线。

## Phase 1：可观测性验收

- [x] 2.1 在同一设备、C:/D: 范围、Release 构建和固定数据快照上完成当前 HEAD 三轮完整应用级基线。
  - **自测标准**：归档 operation/scope 指标、发现数、可搜索数、代表性查询和三轮中位数；历史数据仅作背景。
- [x] 2.2 在真实 Tauri/WebView 中采集索引和查询期间的输入延迟 p95。
  - **自测标准**：保存原始样本、采样方法和 p95；后端查询或 jsdom 不得替代真实交互数据。

## Phase 2：索引正确性

- [x] 3.1 为 `SearchIndexEntry` 和 Tantivy schema 增加规范化 exact `scope_key`，实现按 scope 删除和重建。
  - **自测标准**：整卷/目录根、同名路径、大小写规范化、删除隔离和 merge 后语义测试通过。
- [x] 3.2 实现可回滚的 schema 迁移及 active/staging/previous 原子发布，切换前核对逐 scope 计数和代表性查询。
  - **自测标准**：迁移、计数不一致拒绝发布、staging/切换失败和 previous 回滚测试通过，旧 active 与 SQLite 保持可用。
- [x] 3.3 在当前 HEAD 上重测单范围启动恢复；只有实测未达标时才实施进一步优化。
  - **自测标准**：固定快照连续三轮，目标范围无旧路径、无受影响范围丢失，并记录完整 operation 报告。

## Phase 3：可靠性验证

- [x] 4.1 补充 active/staging/previous 切换的 operation ID、阶段、路径、目录状态、并发查询、句柄诊断和重试日志。
- [x] 4.2 在并发查询、遗留目录和外部占用场景完成 Windows 真机多轮复现与回归。
- [x] 4.3 验证切换失败后旧 active 查询、UI 错误反馈和重启恢复。
  - **自测标准**：持续占用超过重试窗口时返回可定位错误，不伪报成功且不遗留不可判定目录状态。

## Phase 4：条件性性能优化与发布校验

- [x] 5.1 仅针对结构化基线确认的真实瓶颈实施 provider 生命周期优化；若门槛已满足，记录“不需要优化”。
  - **判定**：provider 生命周期和持久化中位数未稳定超过五分钟，本 change 不实施性能优化，详见 `performance-decision.md`。
- [x] 5.2 对每个实际优化使用同一夹具完成前后三轮比较，确认结果、权限、UI 响应、其它 scope 查询和回滚语义无回退。
  - **判定**：5.1 未实施实际优化，本项无前后优化样本；现有三轮基线和可靠性测试继续作为回归证据。
- [x] 5.3 运行 Rust 测试、Clippy、格式、前端测试、构建和 `openspec validate optimize-volume-indexing-followups --strict`。
  - **验证结果**：Rust 175 passed/9 ignored，Clippy `-D warnings`、Rust 格式、前端 111 passed、TypeScript/Vite build、ESLint 和 OpenSpec strict validation 均通过。
