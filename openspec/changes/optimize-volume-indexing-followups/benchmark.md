# 当前 HEAD 多卷应用级基线

## 环境与方法

- 日期：2026-07-31
- 构建：`cargo test --release --lib`
- 测试：`search::tests::windows_multi_volume_application_rebuild_performance`
- 搜索范围：`C:\`、`D:\`
- 每轮使用独立临时目录、真实 `FileSearchManager.start()`、真实 Index Service、watcher、共享 staging Tantivy writer 和后台 SQLite/USN 持久化。
- 测试主体完成后，当前测试进程的外层 PowerShell 包装偶发不退出；输出已在 `NTFS_APP_PHASE` 行完整打印，包装进程随后被终止，不影响已打印样本数据。

## 三轮结果

| 样本 | 调度 | 首批可搜索 | 全部查询 ready | 持久化完成 | 发现记录 | 可搜索文件 | C 查询 | D 查询 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 105 ms | 14,143 ms | 57,615 ms | 299,647 ms | 5,744,991 | 4,932,218 | 33 ms | 1 ms |
| 2 | 104 ms | 13,946 ms | 61,139 ms | 303,882 ms | 5,745,185 | 4,932,385 | 55 ms | 30 ms |
| 3 | 105 ms | 14,930 ms | 62,386 ms | 318,554 ms | 5,745,218 | 4,932,392 | 91 ms | 34 ms |
| 中位数 | 105 ms | 14,143 ms | 61,139 ms | 303,882 ms | — | — | 55 ms | 30 ms |

## 判定

- 首批可搜索中位数 14.143 秒，满足 30 秒目标。
- 全部查询 ready 中位数 61.139 秒，超过 60 秒目标 1.139 秒；当前不能判定该门槛稳定满足。
- 持久化中位数 303.882 秒，超过 5 分钟目标 3.882 秒；当前不能默认跳过生命周期优化。
- 三轮均命中 C/D 代表性查询，且最终 provider 可搜索计数与 `indexed_files` 一致。
- 第三轮收尾阶段出现三次 `count-mismatch database=4932392 query=4932401`；需要在索引正确性/可靠性阶段查明并增加一致性验收，不能将该现象视为无害日志。

## 限制

- 当前 `IndexOperationSnapshot` 尚未通过应用事件或基准报告导出 operation ID；本文件的阶段数据来自同一测试的结构化 provider 状态和 `NTFS_APP_PHASE` 输出，后续需补齐 operation 快照报告。
- 当前会话没有真实 WebView 键入采样入口，因此 UI 输入 p95 尚未验收。
