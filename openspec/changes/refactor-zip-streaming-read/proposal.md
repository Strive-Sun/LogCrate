# Change: zip 条目真流式读取,消除整条目入内存

## Why
基线要求 `open_entry` 返回可流式读取的解压流、Stored 条目可直接 seek。但当前实现把整个条目一次性读入内存 `Vec<u8>` 游标(`zip_reader.rs`),几百 MB 的大条目会造成等量内存峰值,与"免感体验/流畅查看 GB 级"目标冲突,也削弱了首屏即时性。需要改为真正的流式读取,并对 Stored 条目支持基于归档偏移的随机 seek。

## What Changes
- `open_entry` 改为返回**流式**解压读取器,不再将整条目缓冲进内存
- 区分 **Stored**(未压缩)与 **Deflate**(压缩)条目:Stored SHALL 支持对归档直接 seek 的随机访问;Deflate 为顺序流,随机访问所需的临时缓存由查看层按需引入
- 保持既有安全边界(实际字节熔断、加密条目明确错误)不回退

## Impact
- Affected specs: `archive-reading`(MODIFIED:免解压读取)
- Affected code:
  - `src-tauri/src/archive/zip_reader.rs`(流式读取器、Stored seek)
  - `src-tauri/src/archive/mod.rs`(必要时扩展 trait 以暴露可 seek 能力)
  - `src-tauri/src/index.rs`(利用流式/可 seek 能力构建索引)
