# 多卷索引性能报告

## 环境与方法

- 日期：2026-08-17；Windows 参考设备，8 核/16 逻辑处理器。
- 搜索范围：真实 NTFS `C:\`、`D:\`；每轮总 MFT 记录约 580.50 万，处于 588 万 ±10% 的绝对门槛范围。
- 构建与入口：`cargo test --release --manifest-path src-tauri/Cargo.toml windows_multi_volume_application_rebuild_performance -- --ignored --nocapture --test-threads=1`。
- 每轮使用独立临时数据库和 Tantivy 索引、真实 `FileSearchManager.start()`、已安装且保持 Running 的 LogCrate Index Service、真实 MFT/USN、watcher、查询发布、后台 SQLite/USN 持久化及事件交接。
- 固定资源预算：卷 worker `W=4`，runnable window `Q=8`，Tantivy 固定 4 个 writer 线程/280 MB 总内存预算，ASCII 文档构建最多 8 个临时 worker；这些上限均不随卷数 N 增长。

## 当前实现三轮结果

时间均从同一 operation 的单调时钟起点计算；`scheduled` 是所有卷离开 pending 的应用调度耗时，`ready delay` 是最后一个查询快照完成至全局 `ready` 发布的延迟。

| 样本 | scheduled | D MFT 枚举 | C MFT 枚举 | 首批可搜索 | 全部查询 ready | ready delay | converged | 发现记录 | 可搜索文件 | C/D 查询 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 106 ms | 15,347 ms | 19,375 ms | 24,970 ms | 56,372 ms | 38 ms | 187,752 ms | 5,805,042 | 4,987,667 | 42/25 ms |
| 2 | 104 ms | 15,542 ms | 19,311 ms | 24,937 ms | 54,934 ms | 37 ms | 190,104 ms | 5,805,039 | 4,987,664 | 42/22 ms |
| 3 | 104 ms | 15,334 ms | 19,298 ms | 24,599 ms | 55,682 ms | 36 ms | 191,444 ms | 5,805,022 | 4,987,647 | 42/24 ms |
| **中位数** | **104 ms** | **15,347 ms** | **19,311 ms** | **24,937 ms** | **55,682 ms** | **37 ms** | **190,104 ms** | — | — | **42/24 ms** |

三轮 operation 均为 `search-1`（独立测试进程的 generation 从 1 开始），最终快照均为 `converged`，C:/D: scope 均为 `ready`，代表性完整路径查询均命中。三轮查询 ready 后 SQLite/USN 仍继续持久化，期间查询保持可用且没有重新进入索引动画。

## 基线与改进

- 本任务首次同设备、同 C:/D: 规模的 Release 基线为查询 ready 525,897 ms；C: 路径解析单阶段 323,334 ms，持久化在旧 600 秒等待上限内未收敛。
- 流水线第一轮可完整比较的优化前样本为 D/C 枚举 19,163/24,696 ms、首批 31,589 ms、查询 ready 95,041 ms、converged 182,386 ms、C/D 查询 41/24 ms。
- 当前三轮中位数相对该可比样本：首批结果缩短 21.1%，查询 ready 缩短 41.4%；相对首次基线查询 ready 缩短 89.4%。后台 converged 中位数为 190.104 秒，低于 5 分钟门槛。
- 历史归档 `2026-07-31-optimize-volume-indexing-followups/benchmark.md` 使用更早的索引 schema、生命周期和约 574.6 万记录，仅作为版本演进背景；本报告的回归判定使用本 change 在稳定 identity、按卷 stage 和 ready/converged 语义生效后的同规模基线。

## 目录变化恢复三轮

入口：`windows_directory_change_rebuild_performance`；真实 D: 创建、重命名并写入证明文件，确认 USN 目录变化后执行全量 MFT/查询索引重建。

| 样本 | MFT 记录 | 可搜索文件 | 枚举 | 查询 ready | 证明文件命中 |
|---:|---:|---:|---:|---:|---:|
| 1 | 2,803,159 | 2,521,576 | 9,185 ms | 26,725 ms | 是 |
| 2 | 2,803,159 | 2,521,576 | 8,800 ms | 24,627 ms | 是 |
| 3 | 2,803,159 | 2,521,576 | 7,679 ms | 24,416 ms | 是 |
| **中位数** | **2,803,159** | **2,521,576** | **8,800 ms** | **24,627 ms** | **是** |

恢复中位数低于当前 D: 全量构建路径的 120% 上限，且三轮均在 60 秒内完成并命中证明文件。

## 真实 Tauri/WebView 输入延迟

- 使用隔离 identifier `com.logcrate.searchlatency.acceptance` 的 Release Tauri/WebView2 实例；空配置启动真实 C:/D: 索引，在状态保持 `scanning` 时连续采集 100 次搜索输入到下一帧 UI 更新的原始样本。
- WebView2 user agent：Chrome/Edge 151；采集点为生产 `recordSearchInputLatency`，不是 jsdom 或后端查询耗时。
- 结果：100 个样本，p95 `16.7 ms`，满足 `≤100 ms`。验收后隔离应用与 9337 调试端口均已停止，Roaming/Local 两个隔离数据目录均已删除，正式 `com.logcrate.app` 配置未被读取或修改。

## 门槛判定

| 门槛 | 中位数/结果 | 判定 |
|---|---:|---|
| 所有卷离开 pending ≤2 s | 104 ms | 通过 |
| 每卷 MFT 枚举 ≤20 s | D 15.347 s；C 19.311 s | 通过 |
| 首批可搜索 ≤30 s | 24.937 s | 通过 |
| 全部卷查询 ready ≤60 s | 55.682 s | 通过 |
| ready 发布延迟 ≤2 s | 37 ms | 通过 |
| 持久化与事件交接 converged ≤5 min | 190.104 s | 通过 |
| UI 输入响应 p95 ≤100 ms | 16.7 ms / 100 样本 | 通过 |
| 目录变化恢复 ≤对应全量构建 120% | 24.627 s | 通过 |
