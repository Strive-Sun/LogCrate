# 归档安全与 Windows 性能测试

## 自动化安全测试

归档限制通过 `ArchiveLimits` 注入。生产配置保持以下默认值：

- 最大嵌套深度：5 层；
- 单条日志及整条嵌套链最大实际解码量：2 GiB；
- 单个归档最大条目数：100,000；
- 条目路径最大长度：4096 字节。
- 单个归档扫描输入/解码量：4 GiB；
- 单次条目清单扫描时间：30 秒。

单元测试使用较小阈值验证相同失败路径，不需要在开发机生成真正的 2 GiB 解压炸弹。执行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml archive::
```

覆盖范围包括：

- 声明后缀与 magic 冲突、伪造 TAR 和伪造嵌套归档；
- 同格式与跨格式惰性嵌套、深度上限、累计解码字节上限；
- 条目数量、路径长度、绝对路径、父级路径和 Windows 盘符路径；
- ZIP/7z 符号链接或 reparse point，TAR 目录、链接和设备节点；
- 截断压缩流、RAR4/RAR5、加密与分卷错误路径；
- 成功、失败和父归档变化后的嵌套缓存清理；
- 索引取消、关闭和 LRU 回收后的日志缓存清理。

## Windows 性能基线

性能基线使用 release 构建并走完整链路：归档识别、条目解码、首批行发布、行索引、索引缓存以及关闭清理。夹具和运行时缓存位于系统临时目录，测试完成后删除。

```powershell
.\scripts\windows-archive-baseline.ps1
```

可调整数据规模和内存门槛：

```powershell
.\scripts\windows-archive-baseline.ps1 -SizeMiB 512 -MaxMemoryMiB 192
```

默认场景：

- 256 MiB 裸日志；
- ZIP Deflate；
- tar.gz；
- tar.zst；
- ZIP → tar.gz 跨格式嵌套。

脚本每 10 ms 采样当前测试进程工作集，并记录：首批行延迟、总耗时、解码/索引吞吐、进程 CPU 时间、工作集增量、索引或嵌套缓存峰值和最终行数。默认报告写入被 Git 忽略的 `src-tauri/target/performance/windows-archive-baseline.md`。

当前自动验收条件：

- 每个场景完整解码并索引目标字节数；
- 行数与 256 字节合成日志记录一致；
- 工作集增量不超过 192 MiB；
- 关闭会话后索引缓存为零；
- 释放嵌套读取链后无中间归档残留。

合成日志具有较高压缩率，当前结果主要衡量解码和索引上限，不代表不可压缩生产日志的磁盘读取速度。7z solid 与大型 RAR 的真实性能需要后续使用可公开再分发的大型夹具单独记录。
