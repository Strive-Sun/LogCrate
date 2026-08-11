## MODIFIED Requirements
### Requirement: 全局文件搜索默认后台可用
系统 SHALL 在缺少搜索配置时默认启用本地文件搜索，并在应用启动后于后台初始化索引；初始化失败不得阻塞主窗口，用户仍可看到可诊断状态并重试。

#### Scenario: 首次启动
- **WHEN** 用户首次启动且不存在 file-search.json
- **THEN** 搜索在后台开始初始化，UI 保持可交互并可在搜索栏查询已完成部分

#### Scenario: 索引数据库损坏
- **WHEN** 搜索数据库报告损坏
- **THEN** 系统隔离损坏缓存、创建新数据库并继续扫描，不向用户报告无法恢复的致命错误

#### Scenario: 文件实时变化
- **WHEN** 监听范围内文件被创建、修改或删除
- **THEN** notify 事件在有界时间内增量更新 SQLite 与 Tantivy 索引，后续搜索反映最新状态
