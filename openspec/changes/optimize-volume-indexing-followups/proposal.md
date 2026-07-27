# Change: 优化多卷索引后续验证与恢复

## Why

当前并行索引已经能够完成索引、报告完整耗时并返回搜索结果，但真实应用中的 provider 生命周期仍可能超过五分钟。原变更还缺少修改前应用级基线和真实 WebView 输入延迟数据；Windows 索引切换与单卷启动恢复也需要生产级验证。

## What Changes

- 以真实应用/UI 语义分别采集 provider 总耗时、查询可用耗时、持久化耗时、结果数和状态收敛时间，并优化超过五分钟的生命周期。
- 建立可复现的修改前全应用基线，并采集真实 WebView 输入响应 p95。
- 设计按卷可寻址的 Tantivy 文档或 segment，改善单卷启动恢复并以 60 秒内完整可搜索为目标。
- 为 Windows active/staging/previous 切换增加生产诊断、遥测和 `os error 5` 复现验证。

## Impact

- Affected specs: `file-search`
- Affected code: 索引协调器、Tantivy schema/segment 管理、Windows 索引切换、UI 性能遥测与基准工具
- 不改变当前已验证的搜索结果和状态语义；五分钟目标是后续优化目标，不宣称本变更已达标。
