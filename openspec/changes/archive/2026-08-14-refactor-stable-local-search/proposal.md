# Change: 重构稳定的本地文件搜索

## Why
当前搜索默认关闭，启动初始化被延迟，而且 SQLite/MFT 介质损坏会让整个搜索模块失败。用户需要可用的本地文件搜索，并允许较慢的首次索引换取稳定性。

## What Changes
- 首次安装默认启用搜索，并在应用启动后台初始化，不阻塞 UI 交互交接。
- 检测到损坏的搜索数据库时隔离损坏文件并自动创建干净数据库，保留可诊断状态。
- 继续使用 notify 监听新增、修改、删除事件并增量更新 Tantivy/SQLite 索引。

## Impact
- Affected specs: file-search
- Affected code: src-tauri/src/search.rs, src-tauri/src/lib.rs
