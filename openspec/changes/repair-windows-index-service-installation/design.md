## Context

Windows 安装包已经携带 `logcrate_index_service.exe`。MSI custom action 使用系统权限并以 `Return="check"` 处理失败，但 NSIS post-install hook 仅把非零退出码写入详情日志，仍允许安装结束。应用内修复命令调用 `open_service`/`start`，因此服务注册缺失或 ACL 损坏时无法恢复；`open_service` 上的统一上下文还掩盖了真实 Win32 错误阶段。

## Goals / Non-Goals

- Goals: 安装失败可见且不可伪装成功；显式 UAC 重新注册可恢复缺失/损坏服务；错误阶段可诊断；GUI 保持普通权限；兼容扫描在失败时继续可用。
- Non-Goals: 不提升主程序；不允许任意进程启动；不修改服务协议或索引数据；不在本 change 建立 Authenticode 证书、企业部署策略或自动静默提权。

## Decisions

- NSIS post-install 执行服务 `--install` 后检查所有非零或执行器错误结果，显示包含退出结果的阻断对话框并中止成功路径。不得只使用 `DetailPrint`。MSI 保留 deferred、`Impersonate="no"`、`Return="check"` 的失败回滚语义。
- 应用内修复始终来自用户显式点击。Rust 从当前 GUI 可执行文件的父目录派生唯一允许的兄弟文件 `logcrate_index_service.exe`，拒绝文件缺失；使用 Windows `ShellExecuteExW` 的 `runas` verb、固定参数 `--install`、无用户输入，等待提升进程退出并检查退出码。
- 提权进程退出码为零后，普通权限进程重新查询服务、启动（若需要）并进行协议握手；只有握手成功才返回修复成功，随后前端重新开始索引。
- 将服务操作按阶段分类：`missing`（SCM 1060）、`accessDenied`（Win32 5）、`startFailed`、`notReady`、`protocolMismatch`、`elevationCancelled`（Win32 1223）、`repairExecutableMissing` 和 `repairFailed`。内部可以使用 Rust enum；Tauri 失败结果至少携带稳定代码与可读消息，provider 降级文案必须保持阶段差异。
- UAC 被拒绝或取消时只返回可重试错误，不自动再次提示；服务不可用时现有 folder-scan 降级继续运行。
- Windows 原生调用封装在小型、可测试边界中。错误码分类、固定路径/参数构造和退出状态解释使用无副作用单元测试；真正 UAC 与服务注册使用 Windows 安装产物验收。

## Risks / Trade-offs

- 未做 Authenticode 签名时 UAC 可能显示未知发布者，企业策略也可能继续拒绝运行 → 明确报告提升/修复失败，不把它归类为服务缺失；证书发布另行规划。
- NSIS 中止不具备 MSI 完整事务回滚能力 → 至少不得显示成功或自动启动应用；错误信息指导用户关闭安全软件拦截后重试，产物验收核对失败路径。
- 等待提升进程会占用修复命令的后台 worker → 必须在阻塞线程执行，不阻塞 WebView/Tauri 异步命令线程，并保持按钮忙碌状态。
- 服务已经存在但 ACL 或二进制路径损坏 → 统一执行 `--install` 的现有更新路径，停止旧服务、更新配置/DACL并重新启动，比只调用 `start` 更可靠。

## Migration Plan

无需索引或配置迁移。新安装通过强化后的安装器注册服务；已有缺失/损坏服务的安装可在搜索设置中点击修复并批准 UAC。卸载仍使用现有服务删除钩子。

## Open Questions

- 无。若真实企业策略要求 Authenticode，作为独立发布安全 change 处理，不弱化本 change 的错误报告和兼容扫描。
