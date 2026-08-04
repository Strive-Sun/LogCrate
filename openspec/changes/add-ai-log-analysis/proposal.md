# Change: 增加日志选区 AI 分析与供应商配置

## Why

用户需要将选中的日志交给 AI 快速梳理，了解日志包含的信息、警告、错误和可能原因。当前日志正文没有 AI 分析入口，也没有安全的供应商和 API Key 配置能力。

## What Changes

- 在日志正文选区右键菜单增加“AI 分析”。
- 增加 OpenAI 兼容接口供应商配置，支持供应商名称、API 请求地址、协议、地址模式、模型和 API Key。
- 同时支持 OpenAI Chat Completions 与 OpenAI Responses API；API 请求地址可作为基础地址自动拼接协议路径，也可作为完整 URL 使用。
- 默认仅允许 HTTPS 和本机 HTTP；用户可对明确配置的内网 HTTP 供应商单独开启不安全连接，并在保存与发送前看到风险警告。
- API Key 只保存到 Windows Credential Manager 或 macOS Keychain，不写入仓库、localStorage、普通配置文件、日志或错误信息。
- AI 分析仅在用户主动点击并确认后发送选中文本；界面显示目标端点、模型和文本规模。
- 增加分析结果面板，分段展示摘要、日志信息、警告、错误、可能原因和建议。

## Impact

- Affected specs: `application-settings`, `log-viewing`
- Affected code: React 设置面板和日志上下文菜单、Tauri IPC、Rust 系统密钥链与协议感知 HTTP 客户端
- 安全边界：选中的日志内容会按用户明确操作发送给所配置的第三方端点；API Key 不进入前端持久化、Git 或应用日志。
