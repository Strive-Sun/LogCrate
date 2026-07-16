# Change: 补齐后端单元测试覆盖

## Why
后端(归档读取、目录监控、行索引/查看)当前没有任何自动化测试,`cargo test` 形同空跑,回归全靠人工。核心解析逻辑(行偏移索引、CRLF 切分、编码解码、超长行截断、类型判定、持久化读写、缓存清理)一旦改动无从守护。补齐单元测试以固化 M1 已实现行为并支撑后续重构。

## What Changes
- 为 `archive-reading` 增加单元测试:列条目仅读清单不解压、打开条目、passthrough、非文本条目标记、声明大小与实际不符时熔断
- 为 `directory-monitoring` 增加单元测试:类型判定(zip/裸文本)、后缀筛选、配置持久化读写、失效目录跳过
- 为 `log-viewing` 增加单元测试:行偏移索引正确性、边界行读取、CRLF/LF 切分、编码解码(UTF-8/GBK/UTF-16 BOM)、超大单行截断、会话关闭与缓存清理
- 将测试纳入 CI(`cargo test` 真正执行用例)

## Impact
- Affected specs: `archive-reading`、`directory-monitoring`、`log-viewing`(各新增"测试覆盖"质量要求)
- Affected code:
  - `src-tauri/src/archive/`(新增 `#[cfg(test)]` 模块与测试夹具)
  - `src-tauri/src/watcher.rs`
  - `src-tauri/src/index.rs`
  - `.github/workflows/ci.yml`(确保 `cargo test` 执行)
