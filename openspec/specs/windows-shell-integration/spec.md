# windows-shell-integration Specification

## Purpose
TBD - created by archiving change add-windows-logcrate-context-menu. Update Purpose after archive.
## Requirements
### Requirement: Windows 文本文件右键分析入口

Windows 安装包 MUST 为 LogCrate 支持的固定文本扩展名注册名为“以 LogCrate 分析”的资源管理器右键菜单。菜单命令 MUST 使用当前安装目录中的 LogCrate 可执行文件并以安全引用形式传递所选文件的完整路径；安装或升级失败时不得留下不完整的 LogCrate 注册表项，卸载 MUST 只删除 LogCrate 创建的键，不得改变默认打开方式或其它应用的菜单。

#### Scenario: 右键启动单个文本日志

- **WHEN** 用户在 Windows 资源管理器中对已安装支持扩展名的普通文本文件选择“以 LogCrate 分析”
- **THEN** LogCrate 收到该文件的完整路径，复用现有文件检查与打开流程，并在需要时把文件父目录加入监控后定位并显示文件内容

#### Scenario: 路径包含空格或非 ASCII 字符

- **WHEN** 被选文件路径包含空格、引号边界或非 ASCII 字符
- **THEN** 右键命令仍只传递一个完整且未截断的文件路径，LogCrate 打开与路径直接启动相同的文件

#### Scenario: 卸载或升级清理菜单

- **WHEN** 用户卸载 LogCrate 或安装新版本替换旧版本
- **THEN** 安装器只移除或重建 LogCrate 自己的右键键值，默认文件关联、其它应用菜单和用户文件保持不变

### Requirement: 启动参数与单实例转发

Windows 桌面应用 MUST 接受最多一个现存普通文件路径作为外部打开请求。首个进程 SHALL 在 WebView 就绪后处理该请求；已有单实例 MUST 将后续请求转发给首个进程、聚焦原窗口并按请求去重，不得创建第二个窗口或丢弃请求。目录、多个文件、选项、失效路径和不支持文件 MUST 整体拒绝且不得产生部分监控或打开操作。

#### Scenario: 首次启动带文件路径

- **WHEN** LogCrate 尚未运行且资源管理器通过右键命令启动它
- **THEN** 应用完成正常启动后自动检查并打开该文件，失败时显示可读错误且不改变监控配置

#### Scenario: 已有实例接收另一个文件

- **WHEN** LogCrate 已有主实例且用户再次执行右键命令
- **THEN** 新进程不创建窗口，主实例收到一次请求、恢复并聚焦原窗口，然后按文件拖入语义处理该文件

#### Scenario: 无效或多余参数

- **WHEN** 外部启动请求包含目录、多个路径、选项、失效路径或不支持的文件
- **THEN** 请求整体被拒绝并显示可读错误或安全忽略，既不添加监控目录，也不创建部分日志会话

