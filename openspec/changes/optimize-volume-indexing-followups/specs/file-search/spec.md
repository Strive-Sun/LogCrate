## MODIFIED Requirements

### Requirement: 结构化索引生命周期观测

系统 SHALL 为每次启动恢复、重建或修复操作分配稳定 operation ID，并以结构化 operation/scope 快照记录 provider 总耗时、查询索引可用、持久化、事件交接、状态收敛、发现数、可搜索数和查询结果数。性能报告 SHALL 使用统一起止点。

#### Scenario: 真实应用性能报告
- **WHEN** 用户在固定设备和数据快照上完成一次 C:/D: 全量索引
- **THEN** 报告通过同一 operation ID 关联全局与逐 scope 阶段，并给出阶段耗时、计数、状态和实际搜索结果。

#### Scenario: 可比基线
- **WHEN** 系统对当前 HEAD 或其同夹具派生版本执行基线
- **THEN** 两组基线使用相同设备、范围、Release 构建、数据快照和 operation 起止点；历史版本数据仅作背景。

### Requirement: 真实 WebView 输入延迟验证

系统 SHALL 从真实 Tauri/WebView 搜索交互采集输入延迟并计算 p95，目标为不超过 100 毫秒；后端查询或 jsdom 延迟不得替代该指标。

#### Scenario: 输入延迟采样
- **WHEN** 用户在索引建立和查询期间连续输入搜索词
- **THEN** 系统记录真实输入到 UI 更新的延迟并报告 p95，且结果与状态提示保持一致。

### Requirement: 单范围启动恢复可搜索

系统 SHALL 在 Tantivy 文档中保存规范化、可精确匹配的 `scope_key`，并以该字段作为按范围删除和重建的正确性边界。单范围恢复期间其它范围 SHALL 继续通过旧 active 快照查询；新范围数据完成并核对后 SHALL 通过可回滚的 active/staging/previous 切换发布。目标范围的 60 秒要求 SHALL 由当前 HEAD 基线验证，只有实测未达标时才触发额外优化。Tantivy segment MUST NOT 作为唯一范围身份或正确性边界。

#### Scenario: 按范围恢复
- **WHEN** 已有多范围索引且仅一个整卷或目录根需要恢复
- **THEN** 该范围的新路径完整可搜索、旧路径不残留，其他范围继续可查询。

#### Scenario: 按范围迁移失败
- **WHEN** `scope_key` schema 迁移、计数核对或 staging 发布失败
- **THEN** 系统保留旧 active 索引和 SQLite 数据，原有范围继续可查询，并记录可定位的迁移或发布阶段错误。

### Requirement: Windows 索引切换诊断与回滚

系统 SHALL 为 active/staging/previous 切换记录 operation ID、阶段、源路径、目标路径、重试次数、目录状态和并发状态；切换失败时 SHALL 保留旧活动索引可查询并显示可定位错误。已有的重试、merge 等待和回滚机制无需重复实现，除非诊断验证发现缺陷。

#### Scenario: 切换被占用
- **WHEN** Windows 在切换期间存在并发查询或外部文件句柄占用
- **THEN** 系统执行有界重试并记录完整上下文；若仍失败，旧活动索引继续提供查询且 UI 指明失败阶段和路径。

### Requirement: 条件化索引生命周期性能优化

系统 SHALL 仅在当前 HEAD 的可比完整应用基线确认存在回归或真实瓶颈时，依据结构化阶段指标优化 provider 生命周期。优化 MUST NOT 牺牲结果正确性、未受影响范围持续查询、UI 响应、权限边界或失败回滚；若现有门槛已满足，系统 SHALL 记录无需优化。

#### Scenario: 基于已验证瓶颈实施优化
- **WHEN** 完整应用基线确认某个 provider 阶段是主要瓶颈
- **THEN** 系统只针对该瓶颈实施优化，并用相同 operation 指标报告前后三轮结果。

#### Scenario: 基线已满足
- **WHEN** provider 生命周期和现有性能门槛在当前 HEAD 基线中已满足
- **THEN** 不强制实施生命周期优化，并归档基线和“不需要优化”的判定依据。
