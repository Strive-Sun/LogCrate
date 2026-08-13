## ADDED Requirements

### Requirement: 监控目录中的 DOCX 预览文档

系统 SHALL 在 Windows 与 macOS 的统一格式注册表中把结构有效的 `.docx` 识别为可预览文档。目录库存 SHALL 将其显示为可直接打开的文档叶子节点；文件稳定后到达检测 SHALL 将其作为受支持候选，但不得在库存扫描或通知阶段解析完整正文。

#### Scenario: 目录树显示有效 DOCX

- **WHEN** 已加载监控目录包含结构有效的 `.docx`
- **THEN** 目录树显示一个可直接打开的文档叶子节点，不把它显示为可展开普通 ZIP

#### Scenario: DOCX 到达通知

- **WHEN** 新 `.docx` 文件完成稳定性检测、结构校验有效且符合当前后缀筛选规则
- **THEN** 系统将其计入受支持文件到达通知，但不为生成通知解析完整主文档正文

#### Scenario: 无效 DOCX 不冒充预览文档

- **WHEN** 新 `.docx` 只有 ZIP magic 或后缀但 DOCX 包结构无效
- **THEN** 系统不把它标记为可预览文档或发送可打开通知，并保留可诊断的格式无效结果

#### Scenario: DOCX 后缀筛选

- **WHEN** 用户的目录后缀筛选包含或排除 `.docx`
- **THEN** DOCX 节点展示与后续到达通知遵守同一持久化筛选规则，显式拖入当前文件的例外仍沿用现有定位语义
