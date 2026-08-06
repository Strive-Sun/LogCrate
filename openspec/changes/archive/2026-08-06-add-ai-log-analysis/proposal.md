# Change: 增加日志选区 AI 分析与供应商配置

## Why

用户需要将选中的日志交给自定义 OpenAI 兼容端点进行梳理，并围绕结果继续追问。该能力涉及第三方数据发送、系统密钥链、本地敏感历史和本地附件读取，必须以用户明确操作和有界输入为前提。

## What Changes

- 在日志正文选区右键菜单提供“AI 分析”，发送前展示供应商、协议、模型、目标地址和字符规模并取得用户确认。
- 提供 Chat Completions 与 Responses 兼容供应商配置；支持基础地址、完整 URL 和逐供应商不安全 HTTP 授权。
- API Key 只保存到 Windows Credential Manager 或 macOS Keychain，不进入前端持久化、普通配置、Git 或应用日志。
- 在唯一主窗口内提供右侧并排的纯白 AI 工作区，展示结构化首轮分析、多轮消息、加载/错误状态和安全 Markdown 内容。
- 使用 AES-256-GCM 将对话历史加密保存到应用数据目录，随机主密钥只保存到系统密钥链；支持历史列表、恢复、删除单条和清空全部，最多保存 100 条。
- 追问区使用带“+”、多行输入和向上箭头的一体化 composer；Enter 发送、Shift+Enter 换行，输入法组词期间不发送。
- 每轮可明确选择最多 5 个普通本地纯文本或日志文件，每个最多读取 256 KiB；拒绝目录、二进制、压缩归档、不可解码、不可读、重复和超限输入。

## Impact

- Affected specs: `application-settings`, `log-viewing`
- Affected code: React 设置与日志 AI 工作区、Tauri 主窗口控制、AI IPC、协议感知 HTTP 客户端、本地附件读取、系统密钥链和加密历史
- 安全边界: 只有用户主动确认或发送时，选中日志、当前对话和明确添加的附件才会发送到所配置端点；API Key、附件正文和完整 AI 响应不得进入普通配置、localStorage、Git 或应用日志
