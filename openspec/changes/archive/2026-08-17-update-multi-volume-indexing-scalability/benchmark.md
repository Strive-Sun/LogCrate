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
| 1 | 200 ms | 12,740 ms | 16,138 ms | 200 ms | 47,297 ms | 40 ms | 174,215 ms | 5,805,896 | 4,988,432 | 46/26 ms |
| 2 | 168 ms | 13,949 ms | 16,935 ms | 168 ms | 48,535 ms | 54 ms | 166,793 ms | 5,805,875 | 4,988,411 | 43/25 ms |
| 3 | 175 ms | 12,757 ms | 15,012 ms | 175 ms | 47,824 ms | 79 ms | 162,943 ms | 5,805,889 | 4,988,424 | 53/24 ms |
| **中位数** | **175 ms** | **12,757 ms** | **16,138 ms** | **175 ms** | **47,824 ms** | **54 ms** | **166,793 ms** | **5,805,889** | **4,988,424** | **46/25 ms** |

三轮 operation 均为 `search-1`（独立测试进程的 generation 从 1 开始），最终快照均为 `converged`，C:/D: scope 均为 `ready`，代表性完整路径查询均命中。三轮查询 ready 后 SQLite/USN 仍继续持久化，期间查询保持可用且没有重新进入索引动画。

## 基线与改进

- 批准 delta 要求与实施前至少三轮完整应用样本比较。正式历史基线来自 `openspec/changes/archive/2026-07-27-parallelize-volume-indexing/benchmark.md` 的 2026-07-26 Release C:/D: 三轮：调度中位 105 ms、首批可搜索 13,203 ms、全部查询 ready 56,137 ms、ready 发布延迟 39 ms、持久化完成 267,755 ms。
- 当前三轮中位数相对正式历史基线：首批从 13,203 ms 缩短至 175 ms（改善 98.7%），全部查询 ready 从 56,137 ms 缩短至 47,824 ms（改善 14.8%），因此分别满足“不回退超过 10%”与“必须低于基线”。后台 converged 从 267,755 ms 缩短至 166,793 ms（改善 37.7%）。
- 本任务首次同设备、同 C:/D: 规模的诊断基线为查询 ready 525,897 ms；C: 路径解析单阶段 323,334 ms，持久化在旧 600 秒等待上限内未收敛。流水线第一轮可完整比较的中途样本为首批 31,589 ms、查询 ready 95,041 ms；这些数据只说明优化过程，不替代上述正式实施前三轮基线。
- 当前 D/C MFT 中位吞吐约 22.0/18.6 万条/秒；D/C 路径解析中位吞吐约 19.2/17.2 万个可搜索文件/秒，均高于 15 万和 8 万的规模归一化门槛。
- 按卷私有 stage 在建表前采用 32 KiB SQLite page size，逐批事务、generation 隔离、取消/失败回滚和 W=4/Q=8 资源边界保持不变。尝试过的独占锁模式因会抢先阻塞未完成 stage 的主动拒绝诊断而撤回，没有进入最终实现或最终三轮。

## 目录变化恢复三轮

入口：`windows_directory_change_rebuild_performance`；真实 D: 创建、重命名并写入证明文件，确认 USN 目录变化后执行全量 MFT/查询索引重建。

| 样本 | MFT 记录 | 可搜索文件 | 枚举 | 查询 ready | 证明文件命中 |
|---:|---:|---:|---:|---:|---:|
| 1 | 2,803,455 | 2,521,873 | 8,943 ms | 25,178 ms | 是 |
| 2 | 2,803,455 | 2,521,873 | 7,045 ms | 23,335 ms | 是 |
| 3 | 2,803,455 | 2,521,873 | 7,084 ms | 23,406 ms | 是 |
| **中位数** | **2,803,455** | **2,521,873** | **7,084 ms** | **23,406 ms** | **是** |

恢复中位数低于当前 D: 全量构建路径的 120% 上限，且三轮均在 60 秒内完成并命中证明文件。

## 真实 Tauri/WebView 输入延迟

- 使用隔离 identifier `com.logcrate.searchlatency.acceptance` 的 Release Tauri/WebView2 实例；空配置启动真实 C:/D: 索引，在状态保持 `scanning` 时连续采集 100 次搜索输入到下一帧 UI 更新的原始样本。
- WebView2 user agent：Chrome/Edge 151；采集点为生产 `recordSearchInputLatency`，不是 jsdom 或后端查询耗时。
- 结果：100 个样本，p95 `17.5 ms`，采样前后均为 `scanning`，满足 `≤100 ms`。验收后隔离应用与 9337 调试端口均已停止，Roaming/Local 两个隔离数据目录均已删除，正式 `com.logcrate.app` 配置未被读取或修改。

## 门槛判定

| 门槛 | 中位数/结果 | 判定 |
|---|---:|---|
| 所有卷离开 pending ≤2 s | 175 ms | 通过 |
| 每卷 MFT 枚举 ≤20 s | D 12.757 s；C 16.138 s | 通过 |
| 首批可搜索 ≤30 s 且不比 13.203 s 基线回退 10% | 175 ms | 通过 |
| 全部卷查询 ready ≤60 s 且低于 56.137 s 基线 | 47.824 s | 通过 |
| ready 发布延迟 ≤2 s | 54 ms | 通过 |
| 持久化与事件交接 converged ≤5 min | 166.793 s | 通过 |
| UI 输入响应 p95 ≤100 ms | 17.5 ms / 100 样本 | 通过 |
| 目录变化恢复 ≤对应全量构建 120% | 23.406 s | 通过 |
