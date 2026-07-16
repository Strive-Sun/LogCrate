## ADDED Requirements
### Requirement: 归档读取测试覆盖
归档读取层 SHALL 具备自动化单元测试,覆盖列条目、打开条目、裸文本 passthrough、非文本条目判定与安全熔断的关键行为,以在重构时守护正确性。

#### Scenario: 列条目不解压
- **WHEN** 运行针对 `list_archive_entries` 的单元测试
- **THEN** 测试断言仅读取中央目录、返回条目清单且不产生任何解压产物

#### Scenario: 打开 Stored 与 Deflate 条目
- **WHEN** 运行 `open_entry` 的单元测试,分别针对 Stored 与 Deflate 条目
- **THEN** 测试断言返回的解压流内容与原始内容一致

#### Scenario: 裸文本 passthrough 与非文本判定
- **WHEN** 运行 passthrough reader 与 `is_log` 判定的单元测试
- **THEN** 测试断言单文件被视为单条目,且非文本条目被标记为非日志

#### Scenario: 安全熔断
- **WHEN** 运行安全边界的单元测试,输入声明大小与实际不符或超上限
- **THEN** 测试断言读取被熔断并返回明确错误
