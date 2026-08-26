# Change: 添加 Windows“以 LogCrate 分析”文本文件右键入口

## Why

当前用户只能通过拖放、监控目录或应用内文件搜索打开日志。Windows 用户已经习惯从资源管理器右键选择编辑器，希望直接使用 LogCrate 分析文本日志，而不需要先启动应用或配置监控目录。

## What Changes

- 为 Windows 安装包注册“以 LogCrate 分析”右键菜单，覆盖 LogCrate 支持的常见文本日志扩展名，并在卸载时只清理 LogCrate 自己的注册表项。
- 让 LogCrate 接收来自资源管理器的单个文件启动参数；已有实例通过单实例通道接收请求并聚焦原窗口，不创建第二个实例。
- 复用现有拖入文件的后端校验、父目录监控、文本/DOCX 识别、打开和目录定位流程；无效、目录、多余参数或不支持文件不得产生部分操作。
- 增加 Windows 安装配置、Rust/前端回归测试和安装产物检查，保留中英文菜单文案约束。

## Impact

- Affected specs: `windows-shell-integration`（新增能力）、`file-drop-handling`（复用既有处理语义）、`application-lifecycle`（启动参数与单实例转发）。
- Affected code: `src-tauri/src/lib.rs`、启动请求状态/命令、`src/App.tsx`、`src/api/tauri.ts`、Windows NSIS/WiX 安装配置与测试脚本。
- Platform scope: Windows 安装包与 Windows 运行时；macOS 保持现有拖入和启动行为不变。
