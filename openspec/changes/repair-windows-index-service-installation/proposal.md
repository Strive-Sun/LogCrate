# Change: 修复 Windows 索引服务安装与提权修复

## Why

全新 Windows 机器可能在主程序安装成功后发现 `LogCrateIndex` 服务并未注册，应用只能把所有 NTFS 卷降级为兼容目录扫描。当前 NSIS 钩子吞掉服务安装失败，“修复 NTFS 索引服务”又只会启动已存在的服务，且同一条错误文案混合了服务缺失、权限拒绝和启动失败，用户无法定位或自行恢复。

## What Changes

- NSIS 安装服务失败时显示阻断性错误和退出码，不再把安装报告为成功；MSI 继续通过 deferred elevated custom action 在失败时中止并回滚。
- “修复 NTFS 索引服务”改为用户主动触发的原生 Windows `runas` 流程，以固定安装目录中的服务二进制和固定 `--install` 参数重新注册、刷新 ACL、启动并完成协议握手；LogCrate GUI 始终保持普通用户权限。
- 明确区分服务未安装、访问被拒绝、启动失败、启动后 IPC 未就绪/协议不兼容以及用户取消 UAC，并在兼容扫描仍可用时保留可重试操作。
- 修复成功后自动重新开始索引；UAC 取消或修复失败时不得伪报成功、不得反复弹出提权，也不得破坏兼容目录扫描。
- 增加 Windows 服务错误分类、固定提权命令、前端修复状态和安装器产物的回归验证，并要求真实 Windows 安装/修复验收。

## Impact

- 受影响规格：`file-search`。
- 受影响代码：`src-tauri/windows/nsis-hooks.nsh`、`src-tauri/windows/index-service.wxs`、`src-tauri/src/ntfs/ipc.rs`、`src-tauri/src/lib.rs`、Windows 依赖特性、`src/api/`、`src/components/FileSearchPanel.tsx`、国际化、相关测试与发布产物检查。
- 权限边界变化：只在用户点击修复时显示一次系统 UAC；提升后的子进程只能运行安装目录中的 `logcrate_index_service.exe --install`，前端不能提供路径或参数。
- 不改变 MFT/USN 协议、搜索结果、索引 schema、macOS 行为或 updater 签名信任；本 change 不引入 Authenticode 证书签发流程。
