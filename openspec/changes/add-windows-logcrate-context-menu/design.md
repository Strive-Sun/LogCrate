## Context

资源管理器右键命令只能传递文件路径，且 Windows 可能先启动一个新进程，再由单实例插件把参数转给已有进程。LogCrate 当前已经有单文件拖入处理链，但没有可复用的启动请求入口。

## Goals / Non-Goals

- Goals: 提供可卸载的 Windows 文本文件右键入口；安全接收一个绝对文件路径；与拖入处理保持同一校验和打开语义；已有实例只处理一次请求。
- Non-Goals: 不把 LogCrate 注册为默认文件关联；不修改用户现有打开方式；不支持多文件批量打开、目录右键、shell 扩展 DLL 或 macOS Finder 服务。

## Decisions

- Decision: 使用 `SystemFileAssociations\\.<ext>\\shell\\LogCrateAnalyze\\command` 注册菜单，而不是覆盖 `*\\shell` 或默认关联。扩展名集合集中定义，避免二进制文件误显示菜单；菜单显示名为“以 LogCrate 分析”，英文界面仍由 Windows shell 注册的固定中文产品入口保持一致。
- Decision: NSIS 通过安装/卸载钩子写入和删除注册表；WiX 使用独立 Component 并由 `componentRefs` 纳入 MSI。两种安装包使用相同扩展名、动词、显示名、图标和命令模板。
- Decision: Rust 在进程入口收集候选参数，在单实例回调中把新参数追加到有界队列并唤醒主窗口；前端通过一次性 `take_open_file_requests` 命令先取启动队列，再监听 `open-file-request` 事件，统一调用现有拖入处理计划。队列只保留规范化后的单文件路径，重复路径按请求去重。
- Decision: 路径处理复用 `inspect_dropped_file` 的后端检查，并增加“启动请求必须是单个普通文件”的边界；失败时前端显示现有可读错误，不添加监控、不创建查看会话。参数解析忽略选项和工作目录，只接受存在的文件路径，防止把安装器控制参数当作日志。

## Risks / Trade-offs

- 注册表菜单在 per-machine 安装下需要管理员权限；安装失败必须使安装器失败而不是留下半套注册表项。
- 新旧版本升级时旧扩展名集合可能残留；卸载和升级脚本只删除带有 LogCrate 动词的键，不触碰用户其它值，并在安装检查中验证集合一致。
- 单实例事件可能早于 WebView 监听，因此 Rust 队列与事件双通道都保留，前端取队列后再监听并按请求 ID 去重。

## Migration Plan

安装或升级 Windows 安装包时写入菜单；卸载时删除对应键。运行中的旧实例通过单实例回调接收新请求；不需要迁移用户配置或日志数据。

## Open Questions

- 无。首版扩展名集合和“只接受单文件”的边界在 proposal 中固定；后续如需自定义扩展名或多选文件另建 change。
