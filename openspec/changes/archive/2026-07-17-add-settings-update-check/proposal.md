# Change: 新增设置面板与应用自更新

## Why

右上角设置按钮目前只是无行为的占位符，用户无法在应用内确认当前版本，也无法获知或安装 GitHub 上的新版本。需要提供一个轻量设置入口，把更新检查分为可持久化的启动自动检查与用户主动触发的手动检查，并在用户确认后完成带进度反馈的签名更新安装。

## What Changes

- 将右上角设置按钮接通为设置面板，展示当前应用版本。
- 提供“启动时自动检查更新”开关，默认开启并持久化用户选择。
- 应用启动时在开关开启的情况下检查一次最新正式版本；仅发现新版本时主动提示用户。
- 提供手动“检查更新”操作，并明确展示检查中、已是最新版、发现新版本和检查失败状态。
- 发现新版本时允许用户选择“跳过此版本”或“下载更新”；跳过后不再自动提示同一版本。
- 下载更新时展示进度条，下载完成后自动验证签名、安装更新并重启应用。
- 将 Release 工作流升级为生成并发布 Tauri updater 清单、签名和各平台更新包。

## Impact

- Affected specs: `application-settings`、`application-updating`（均新增）
- Affected code: `src/components/TopBar.tsx`、新增设置/更新组件、`src/App.tsx`、`src/api/*`、Tauri 插件配置、Release workflow、前端样式与测试配置
- External dependency: Tauri updater/process 官方插件及 GitHub Releases 更新源；不引入应用服务端或账号体系
- Network behavior: 仅在自动检查开启且应用启动时，或用户手动检查时发起请求
- Release prerequisite: 仓库需要配置 Tauri updater 私钥及密码 secrets；应用只内置对应公钥，私钥不得进入仓库
