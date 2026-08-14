## Context
搜索索引是可重建缓存，不应因为 SQLite WAL 或 FTS 损坏阻止应用启动。

## Decisions
- 保留现有 Tantivy 查询索引和 notify 增量监听，避免引入未经验证的新依赖。
- 默认配置仅对缺失配置生效；用户已经显式关闭搜索的配置保持关闭。
- 数据库损坏时将 sqlite、wal、shm 文件移动到带时间戳的 quarantine 文件名，再重新初始化。

## Risks / Trade-offs
- 损坏数据库会丢失旧索引，但索引会由后台扫描重建；原始用户文件不受影响。
