## ADDED Requirements

### Requirement: AI 供应商配置与密钥安全

系统 SHALL 提供 OpenAI 兼容接口供应商的名称、API 请求地址、协议（Chat Completions 或 Responses）、地址模式（基础地址或完整 URL）、模型和逐供应商不安全 HTTP 授权配置；API Key MUST 仅保存于当前平台系统密钥链，前端、localStorage、普通配置文件、Git 和应用日志中不得出现明文 API Key。系统 MUST 默认拒绝非本机 HTTP 地址，且仅在用户对当前供应商明确确认风险后允许该供应商使用已保存的 HTTP 端点。

#### Scenario: 保存供应商密钥

- **WHEN** 用户在设置中输入供应商信息和 API Key 并保存
- **THEN** 系统将非敏感配置保存到应用设置，将 API Key 写入系统密钥链，并仅返回已配置状态

#### Scenario: 删除供应商

- **WHEN** 用户删除已配置供应商
- **THEN** 系统删除对应系统密钥链条目和非敏感配置，且后续读取不会返回旧密钥

#### Scenario: 密钥链不可用

- **WHEN** 系统密钥链不可用或访问被拒绝
- **THEN** 系统显示明确错误，不回退到明文存储，也不将密钥写入日志

#### Scenario: 配置 Responses API 请求地址

- **WHEN** 用户选择 Responses 协议并填写基础 API 请求地址或完整 URL
- **THEN** 系统保存协议和地址模式，并明确展示最终请求目标的生成方式

#### Scenario: 默认拒绝内网 HTTP

- **WHEN** 用户填写非本机 HTTP API 请求地址但未开启当前供应商的不安全 HTTP 授权
- **THEN** 系统拒绝保存或连接，并提示 API Key 与日志内容缺少 TLS 保护

#### Scenario: 明确允许当前供应商使用 HTTP

- **WHEN** 用户对当前供应商开启不安全 HTTP、阅读风险提示并明确确认
- **THEN** 系统只允许该供应商使用其已保存的 HTTP 端点，并保留可见的不安全状态，不放行其它供应商或地址
