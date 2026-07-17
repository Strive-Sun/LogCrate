# application-lifecycle Specification

## Purpose
TBD - created by archiving change add-close-to-tray. Update Purpose after archive.
## Requirements
### Requirement: 关闭主窗口时保持后台运行

系统 SHALL 在用户点击主窗口关闭按钮时阻止窗口销毁并将其隐藏到系统托盘，同时 MUST 保持应用进程、目录监控和已启动后台任务继续运行；普通最小化按钮 SHALL 保持平台默认行为。

#### Scenario: 点击窗口关闭按钮

- **WHEN** 用户点击主窗口标题栏关闭按钮
- **THEN** 主窗口从桌面和任务栏隐藏，LogPeek 进程继续运行且系统托盘保留 LogPeek 图标

#### Scenario: 隐藏期间检测新日志

- **WHEN** 主窗口已隐藏到托盘且监控目录出现新日志
- **THEN** 系统继续完成目录同步和新日志检测，恢复窗口后可看到隐藏期间产生的最新状态

#### Scenario: 使用最小化按钮

- **WHEN** 用户点击主窗口最小化按钮
- **THEN** 系统按平台默认方式最小化窗口，不将其解释为关闭或退出

### Requirement: 从系统托盘恢复主窗口

系统 SHALL 提供托盘“显示主窗口”操作，并在平台支持时允许点击托盘图标恢复同一个主窗口实例；恢复操作 SHALL 显示、取消最小化并聚焦窗口，且 MUST NOT 重建或清空现有前端状态。

#### Scenario: 通过托盘菜单恢复

- **WHEN** 主窗口隐藏且用户选择托盘菜单“显示主窗口”
- **THEN** 系统显示并聚焦原主窗口，目录树、查看会话和未读状态保持不变

#### Scenario: 重复显示已可见窗口

- **WHEN** 主窗口已经可见且用户再次触发托盘显示操作
- **THEN** 系统聚焦现有窗口且不创建第二个窗口实例

### Requirement: 通过系统托盘退出应用

系统 SHALL 在托盘菜单提供“退出 LogPeek”，该操作 SHALL 完整结束应用进程；关闭到托盘逻辑 MUST NOT 阻断托盘退出、自动更新重启或操作系统会话结束。

#### Scenario: 托盘菜单退出

- **WHEN** 用户选择托盘菜单“退出 LogPeek”
- **THEN** 系统结束应用进程并停止目录监控、索引任务和托盘图标

#### Scenario: 自动更新后重启

- **WHEN** updater 完成更新并请求应用重启
- **THEN** 系统允许当前进程退出并启动新版本，不把更新退出误处理为隐藏窗口

#### Scenario: 操作系统结束会话

- **WHEN** 操作系统关机、注销或明确终止应用进程
- **THEN** 系统不以关闭到托盘行为阻止会话结束
