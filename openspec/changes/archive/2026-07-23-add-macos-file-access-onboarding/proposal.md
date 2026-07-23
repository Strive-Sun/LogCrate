# Change: 增加 macOS 文件访问授权引导与持久授权

## Why

LogCrate 在 macOS 启动后恢复监控目录、加载其中的文件时会重复触发文件访问授权，破坏后台监控和快速查看体验。用户希望安装后只完成一次明确的系统授权，后续启动可直接恢复已授权目录；需要访问更广范围时，应用应把用户引导到“完全磁盘访问权限”页面，而不能假装能够绕过 macOS 的安全确认。

## What Changes

- macOS 首次启动（已有用户为升级后首次启动）显示一次文件访问说明，区分“已选择目录的持久访问”和可选的“完全磁盘访问权限”。
- 用户确认前往授权后，直接打开“系统设置 → 隐私与安全性 → 完全磁盘访问权限”；目标深链不可用时降级打开“隐私与安全性”页面并展示手动路径。
- 明确要求权限只能由用户在系统设置中添加并开启；LogCrate 不自动修改 TCC 数据库、不模拟点击、不以辅助功能或安装脚本绕过确认。
- macOS 通过系统目录选择器加入监控目录时创建 security-scoped bookmark，持久化书签并在后续启动恢复访问，使同一目录及系统允许的子目录无需反复选择。
- 书签失效、目录移动或权限撤销时，仅对受影响目录显示可操作的重新授权状态，不阻塞其它目录和应用启动。
- 全盘搜索与文件打开根据实际访问结果工作；未获得完全磁盘访问权限时继续索引已授权/可读范围，并对不可读范围显示有界诊断，不循环弹窗。
- 本轮验收限定为同一次安装且 macOS 未撤销权限的跨重启场景：完成一次授权后，普通启动、恢复监控和打开已授权范围不再重复引导。

## Impact

- Affected specs: `macos-file-access`（new）；与 `directory-monitoring`、`file-search`、`application-lifecycle` 和 `application-branding` 的既有行为衔接
- Affected code: macOS 原生桥接、Tauri command/state、目录选择与监控配置、启动引导 UI、搜索 provider、设置与本地化
- New persistence: 版本化的权限引导状态，以及按监控根保存的 opaque security-scoped bookmark 数据
- Security impact: 应用可在用户明确授权后读取更广的本机路径；书签不得上传、写入日志或暴露给前端业务状态
- Release impact: 本轮不以 Apple Developer ID、签名或公证为实施前置条件，也不承诺重新构建、覆盖升级或重新安装后的系统权限继承；应用内不主动展示升级权限风险提示
- Platform impact: Windows 行为保持不变；所有新增原生能力以 macOS 条件编译隔离
