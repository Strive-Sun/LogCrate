## Context

LogCrate 是 Tauri 2 + React/Rust 桌面应用。日志可能来自本地文件或压缩包，正文使用虚拟列表，只能从当前选区取得文本。项目当前没有 AI 供应商或密钥存储抽象。

## Goals / Non-Goals

- Goals: 提供 OpenAI 兼容接口配置、系统密钥链保存、用户确认后的选区分析、明确的失败和隐私提示。
- Non-Goals: 不自动上传日志、不在后台持续分析、不实现供应商专属协议、不把完整日志文件上传、不承诺 AI 结论正确。

## Decisions

- Decision: 统一使用 OpenAI Chat Completions 兼容请求模型，配置 `name`、`base_url`、`model`，API Key 由 Rust 端读取。
- Decision: 使用系统密钥链（Windows Credential Manager、macOS Keychain）保存密钥；前端只接收是否已配置，不接收或回显明文密钥。
- Decision: AI 请求由 Rust IPC 发起，前端发送选中文本和供应商 ID；Rust 校验端点为 HTTPS（允许本机开发端点），限制文本长度并设置请求超时。
- Decision: 右键“AI 分析”先打开确认/分析面板，展示供应商、模型、端点主机、字符数和“内容将发送到远程服务”提示，确认后才发起请求。
- Decision: 分析结果按结构化 Markdown/JSON 解析为摘要、信息、警告、错误、可能原因和建议；无法解析时以安全纯文本展示。
- Decision: 首批提供 OpenAI、DeepSeek、通义千问和 OpenRouter 预设，同时允许用户添加自定义兼容端点。
- Decision: 远程端点必须使用 HTTPS；仅 `localhost`、`127.0.0.1` 和 `::1` 可使用 HTTP，不允许普通局域网或公网明文 HTTP。

## Risks / Trade-offs

- 第三方服务会获得用户选中的日志内容；通过明确确认、端点展示、长度限制和不自动发送降低风险。
- 系统密钥链在权限或平台 API 不可用时可能失败；配置界面必须显示不可用原因且不回退到明文存储。
- 不同兼容供应商响应格式可能不同；保留原始安全文本回退，不将响应内容写入日志。

## Migration Plan

不迁移已有用户数据。首次使用时没有供应商配置；已有 localStorage 不读取任何 AI Key 字段。删除供应商时同时删除对应系统密钥链条目。

## Open Questions

无。
