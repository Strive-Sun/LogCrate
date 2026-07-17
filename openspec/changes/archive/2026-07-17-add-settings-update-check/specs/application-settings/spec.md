## ADDED Requirements

### Requirement: 设置面板

系统 SHALL 在用户点击顶栏设置按钮时打开设置面板，并允许用户通过再次点击设置按钮、点击面板外区域或按 Escape 关闭面板。

#### Scenario: 打开并关闭设置面板

- **WHEN** 用户点击顶栏设置按钮
- **THEN** 系统显示设置面板及其中的版本和更新选项
- **WHEN** 用户点击面板外区域或按 Escape
- **THEN** 系统关闭设置面板

### Requirement: 当前版本展示

系统 SHALL 在设置面板中展示当前运行构建的应用版本，且该版本 MUST 来自 Tauri 应用元数据而不是前端重复维护的常量。

#### Scenario: 查看当前版本

- **WHEN** 用户打开设置面板
- **THEN** 系统以 `X.Y.Z` 格式展示当前运行版本

