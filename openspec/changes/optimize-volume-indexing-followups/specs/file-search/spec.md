## ADDED Requirements

### Requirement: 结构化索引生命周期观测

系统 SHALL 为每次启动恢复、重建或修复操作分配稳定的 operation ID，并以结构化 operation/volume 生命周期快照分别记录 provider 总耗时、查询索引可用耗时、持久化耗时、事件交接耗时、状态收敛时间、发现记录数、可搜索文件数和查询结果数。性能报告 SHALL 使用统一起止点。

#### Scenario: 真实应用性能报告
- **WHEN** 用户在固定设备和数据快照上完成一次 C:/D: 全量索引
- **THEN** 报告通过同一 operation ID 关联全局与逐卷阶段，分别给出各阶段耗时、计数、状态收敛时间和实际搜索结果，不以单一总耗时或 MFT 子阶段掩盖完整应用瓶颈

#### Scenario: 修改前后基线可比
- **WHEN** 系统对修改前版本和当前版本执行性能基线
- **THEN** 两组基线使用相同设备、搜索范围、Release 构建、固定数据快照和 operation 起止点，且不以单元测试或单卷快速阶段代替完整应用生命周期

### Requirement: 真实 WebView 输入延迟验证

系统 SHALL 从真实 Tauri/WebView 搜索交互采集输入延迟并计算 p95，目标为不超过 100 毫秒；后端查询或 jsdom 延迟不得替代该指标。

#### Scenario: 输入延迟采样
- **WHEN** 用户在索引建立和查询期间连续输入搜索词
- **THEN** 系统记录真实输入到 UI 更新的延迟并报告 p95，且结果与状态提示保持一致

### Requirement: 单卷启动恢复可搜索

系统 SHALL 在 Tantivy 文档中保存规范化、可精确匹配的 `volume_key`，并以该字段作为按卷删除和重建的正确性边界。单卷恢复期间其它卷 SHALL 继续通过旧活动快照查询；新卷数据完成并核对后 SHALL 通过可回滚的 active/staging/previous 切换发布，目标是在 60 秒内达到完整可搜索。Tantivy segment MAY 用于性能优化，但 MUST NOT 作为唯一卷身份或正确性边界。

#### Scenario: 单卷恢复
- **WHEN** 已有多卷索引且仅一个卷需要启动恢复
- **THEN** 该卷在 60 秒内完整可搜索，其他卷继续可查询，旧路径不会残留

#### Scenario: 按卷索引迁移失败
- **WHEN** `volume_key` schema 迁移、逐卷计数核对或 staging 发布失败
- **THEN** 系统保留旧 active 索引和 SQLite 数据，所有原有卷继续可查询，并记录可定位的迁移或发布阶段错误

### Requirement: Windows 索引切换诊断与回滚

系统 SHALL 为 active/staging/previous 索引切换记录阶段、源路径、目标路径、重试次数和并发状态；切换失败时 SHALL 保留旧活动索引可查询并显示可定位错误。

#### Scenario: 切换被占用
- **WHEN** Windows 在切换期间存在并发查询或外部文件句柄占用
- **THEN** 系统执行有界重试并记录完整上下文；若仍失败，旧活动索引继续提供查询且 UI 指明失败阶段和路径

### Requirement: 索引生命周期性能优化

系统 SHALL 仅在架构拆分、结构化观测、可比基线、单卷恢复和 Windows 切换可靠性均完成各自验证后，依据真实应用的结构化阶段指标优化超过五分钟的 provider 生命周期。优化 MUST NOT 牺牲结果正确性、未受影响卷持续查询、UI 响应、权限边界或失败回滚。

#### Scenario: 基于已验证瓶颈实施优化
- **WHEN** 可比的完整应用基线确认某个 provider 阶段是超过五分钟生命周期的主要瓶颈
- **THEN** 系统只针对已证实瓶颈实施和验证优化，并用相同 operation 指标报告修改前后三轮中位数、结果数、状态收敛和 WebView p95

#### Scenario: 前序任务尚未通过
- **WHEN** 架构、观测、基线、单卷恢复或 Windows 可靠性的任一自测尚未通过
- **THEN** 生命周期性能优化不得开始，未通过的任务保持未完成状态
