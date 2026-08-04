## Context

LogCrate 是 Tauri 2 + React/Rust 桌面应用。日志可能来自本地文件或压缩包，正文使用虚拟列表，只能从当前选区取得文本。项目当前没有 AI 供应商或密钥存储抽象。

## Goals / Non-Goals

- Goals: 提供 OpenAI Chat Completions 与 Responses 兼容接口配置、系统密钥链保存、用户确认后的选区分析、明确的失败和隐私提示。
- Non-Goals: 不自动上传日志、不在后台持续分析、不实现 OpenAI 兼容格式之外的供应商专属协议、不把完整日志文件上传、不承诺 AI 结论正确。

## Decisions

- Decision: 供应商配置增加 `protocol`（`chat_completions` 或 `responses`）、`endpoint_mode`（`base` 或 `full`）和 `allow_insecure_http`；继续配置 `name`、`base_url`、`model`，API Key 由 Rust 端读取。旧配置缺少新增字段时默认使用 Chat Completions、基础地址模式且不允许不安全 HTTP。
- Decision: 使用系统密钥链（Windows Credential Manager、macOS Keychain）保存密钥；前端只接收是否已配置，不接收或回显明文密钥。
- Decision: AI 请求由 Rust IPC 发起，前端发送选中文本和供应商 ID；Rust 根据协议和地址模式确定请求 URL：基础地址模式分别拼接 `/chat/completions` 或 `/responses`，完整 URL 模式不再拼接路径；两种协议均使用 Bearer API Key，限制文本长度并设置请求超时。
- Decision: 右键“AI 分析”先打开确认/分析面板，展示供应商、模型、端点主机、字符数和“内容将发送到远程服务”提示，确认后才发起请求。
- Decision: 分析结果按结构化 Markdown/JSON 解析为摘要、信息、警告、错误、可能原因和建议；无法解析时以安全纯文本展示。
- Decision: 首批提供 OpenAI、DeepSeek、通义千问和 OpenRouter 预设，同时允许用户添加自定义兼容端点。
- Decision: HTTPS 与 `localhost`、`127.0.0.1`、`::1` 的 HTTP 默认允许。其它 HTTP 地址默认拒绝；只有用户对当前供应商明确开启 `allow_insecure_http` 并确认风险后才允许，授权仅绑定该供应商和已保存端点，不形成全局 HTTP 放行。
- Decision: 测试连接按供应商协议向实际分析端点发送不含用户日志的最小请求，不再固定依赖 `/models`；Responses API 从 `output[].content[]` 的文本内容提取结果，Chat Completions 保持从 `choices[].message.content` 提取结果。

## Risks / Trade-offs

- 第三方服务会获得用户选中的日志内容；通过明确确认、端点展示、长度限制和不自动发送降低风险。
- 内网 HTTP 会让 API Key 与日志内容在传输链路上缺少 TLS 保护；默认拒绝，并通过逐供应商显式授权、醒目警告和发送前再次展示 HTTP 目标降低误用风险，仍优先建议供应商提供 HTTPS。
- 系统密钥链在权限或平台 API 不可用时可能失败；配置界面必须显示不可用原因且不回退到明文存储。
- 不同兼容供应商响应格式可能不同；保留原始安全文本回退，不将响应内容写入日志。

## Migration Plan

不迁移已有用户数据。首次使用时没有供应商配置；已有 localStorage 不读取任何 AI Key 字段。删除供应商时同时删除对应系统密钥链条目。

## Open Questions

无。
