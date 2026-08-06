//! Secure AI provider configuration primitives.
//!
//! Non-sensitive provider metadata is kept separate from the API key. Keys are
//! stored only in the platform credential store and are never serialized.

#![allow(dead_code)] // Provider commands consume this module in the next task.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

const KEYRING_SERVICE: &str = "com.logcrate.ai-provider";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub key_configured: bool,
    #[serde(default)]
    pub protocol: AiProtocol,
    #[serde(default)]
    pub endpoint_mode: AiEndpointMode,
    #[serde(default)]
    pub allow_insecure_http: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiProtocol {
    #[default]
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiEndpointMode {
    #[default]
    Base,
    Full,
}

const MAX_ANALYSIS_CHARS: usize = 120_000;
const MAX_AI_ATTACHMENTS: usize = 5;
const MAX_AI_ATTACHMENT_BYTES: usize = 256 * 1024;
const SESSION_ID_HEADER: &str = "session-id";
const THREAD_ID_HEADER: &str = "thread-id";
const INITIAL_ANALYSIS_INSTRUCTIONS: &str = r#"你是面向日志排查用户的分析助手。请输出简洁、有效的中文 Markdown，不要把 Markdown 标记包在外层代码块中。

先用不超过 3 句话概括日志来源、运行环境和整体结论。随后严格按原日志出现顺序，将有实际含义的日志合并为事件段逐段解析；重复或同类日志必须合并并标注次数，不要逐行复述。

每个事件段固定使用以下结构：
## N. 事件名称
**日志**
```text
仅引用支撑判断的代表性原始日志
```
**说明**
用 1 至 3 句话说明这段日志表示什么，以及它与前后事件的关系。
**流程**（只有能从日志直接推导时才写）
```text
步骤 A → 步骤 B → 步骤 C
```
**结论**
用一句话标明“正常”“异常”或“无法确定”，异常时只写有日志证据的原因。

只有存在真实警告、错误或风险时，最后增加“## 需要关注”，最多列 3 项，并给出可执行的下一步。不要机械输出“主要信息/警告/错误/可能原因/建议”五个大章节，不要重复相同结论，不要堆砌背景知识。不得臆造日志中不存在的调用链、时间、状态或根因；证据不足时明确写“无法从当前日志确定”。"#;
const FOLLOW_UP_INSTRUCTIONS: &str = r#"你是日志分析助手。请基于原始日志、补充日志和已有对话准确回答用户追问。使用简洁的中文 Markdown；优先引用相关日志代码块并直接给出结论。除非用户要求重新分析，否则不要重复完整报告，不要扩写无关背景，也不得臆造日志中不存在的事实。"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnalysisResult {
    pub provider_id: String,
    pub model: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAttachmentSummary {
    pub path: String,
    pub name: String,
    pub char_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryUpdate {
    pub id: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadedAiAttachment {
    summary: AiAttachmentSummary,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    temperature: f32,
    messages: [ChatMessage<'a>; 2],
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    instructions: &'a str,
    input: &'a str,
    store: bool,
    stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAiProvider {
    id: String,
    name: String,
    base_url: String,
    model: String,
    #[serde(default)]
    protocol: AiProtocol,
    #[serde(default)]
    endpoint_mode: AiEndpointMode,
    #[serde(default)]
    allow_insecure_http: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProviderConfigError {
    EmptyId,
    InvalidId,
    EmptyName,
    EmptyModel,
    InvalidEndpoint,
}

impl std::fmt::Display for AiProviderConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyId => "provider id is required",
            Self::InvalidId => "provider id contains unsupported characters",
            Self::EmptyName => "provider name is required",
            Self::EmptyModel => "provider model is required",
            Self::InvalidEndpoint => "provider endpoint must use HTTPS or localhost HTTP",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AiProviderConfigError {}

impl AiProviderConfig {
    pub fn validate(&self) -> Result<(), AiProviderConfigError> {
        if self.id.trim().is_empty() {
            return Err(AiProviderConfigError::EmptyId);
        }
        if !self
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        {
            return Err(AiProviderConfigError::InvalidId);
        }
        if self.name.trim().is_empty() {
            return Err(AiProviderConfigError::EmptyName);
        }
        if self.model.trim().is_empty() {
            return Err(AiProviderConfigError::EmptyModel);
        }
        if !is_allowed_endpoint(&self.base_url, self.allow_insecure_http) {
            return Err(AiProviderConfigError::InvalidEndpoint);
        }
        Ok(())
    }
}

fn is_allowed_endpoint(value: &str, allow_insecure_http: bool) -> bool {
    let endpoint = value.trim();
    let Some((scheme, remainder)) = endpoint.split_once("://") else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("https") {
        return !remainder.is_empty() && !remainder.contains(char::is_whitespace);
    }
    if !scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    let host = remainder
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_matches(['[', ']']);
    allow_insecure_http || matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn validate_analysis_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Select some log text before starting AI analysis".to_string());
    }
    if text.chars().count() > MAX_ANALYSIS_CHARS {
        return Err(format!(
            "Selected log text is too large (maximum {MAX_ANALYSIS_CHARS} characters)"
        ));
    }
    Ok(())
}

fn attachment_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("未命名文件")
        .chars()
        .map(|character| {
            if character.is_control() {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn is_archive_attachment(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst"
    )
}

fn decode_attachment(bytes: &[u8], name: &str) -> Result<String, String> {
    let decoded = if bytes.starts_with(&[0xff, 0xfe]) {
        let (text, _, had_errors) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        if had_errors {
            return Err(format!("附件无法按文本解码: {name}"));
        }
        text.into_owned()
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        let (text, _, had_errors) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        if had_errors {
            return Err(format!("附件无法按文本解码: {name}"));
        }
        text.into_owned()
    } else {
        if bytes.contains(&0) {
            return Err(format!("附件不是受支持的文本文件: {name}"));
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => text.trim_start_matches('\u{feff}').to_string(),
            Err(_) => {
                let (text, _, had_errors) = encoding_rs::GB18030.decode(bytes);
                if had_errors {
                    return Err(format!("附件无法按文本解码: {name}"));
                }
                text.into_owned()
            }
        }
    };
    let char_count = decoded.chars().count();
    let control_count = decoded
        .chars()
        .filter(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        .count();
    if control_count > 8 && control_count.saturating_mul(20) > char_count {
        return Err(format!("附件不是受支持的文本文件: {name}"));
    }
    Ok(decoded)
}

fn load_ai_attachments(
    selected_text: &str,
    attachment_paths: &[String],
) -> Result<Vec<LoadedAiAttachment>, String> {
    if attachment_paths.len() > MAX_AI_ATTACHMENTS {
        return Err(format!("每次最多添加 {MAX_AI_ATTACHMENTS} 个附件"));
    }
    let mut seen = HashSet::new();
    let mut total_chars = selected_text.chars().count();
    let mut loaded = Vec::with_capacity(attachment_paths.len());
    for requested_path in attachment_paths {
        let requested = Path::new(requested_path);
        let requested_name = attachment_name(requested);
        let canonical = std::fs::canonicalize(requested)
            .map_err(|_| format!("无法读取附件: {requested_name}"))?;
        let name = attachment_name(&canonical);
        if !seen.insert(canonical.clone()) {
            return Err(format!("附件重复添加: {name}"));
        }
        if is_archive_attachment(&canonical) {
            return Err(format!("不支持压缩归档附件: {name}"));
        }
        let metadata =
            std::fs::metadata(&canonical).map_err(|_| format!("无法读取附件: {name}"))?;
        if !metadata.is_file() {
            return Err(format!("附件必须是普通文件: {name}"));
        }
        if metadata.len() > MAX_AI_ATTACHMENT_BYTES as u64 {
            return Err(format!("附件超过 256 KiB 限制: {name}"));
        }
        let mut file = File::open(&canonical).map_err(|_| format!("无法读取附件: {name}"))?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.by_ref()
            .take((MAX_AI_ATTACHMENT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| format!("无法读取附件: {name}"))?;
        if bytes.len() > MAX_AI_ATTACHMENT_BYTES {
            return Err(format!("附件超过 256 KiB 限制: {name}"));
        }
        let content = decode_attachment(&bytes, &name)?;
        let char_count = content.chars().count();
        total_chars = total_chars.saturating_add(char_count);
        if total_chars > MAX_ANALYSIS_CHARS {
            return Err(format!(
                "原始日志与附件合计超过 {MAX_ANALYSIS_CHARS} 个字符限制"
            ));
        }
        loaded.push(LoadedAiAttachment {
            summary: AiAttachmentSummary {
                path: canonical.to_string_lossy().into_owned(),
                name,
                char_count,
            },
            content,
        });
    }
    Ok(loaded)
}

fn attachment_context(attachments: &[LoadedAiAttachment]) -> String {
    attachments
        .iter()
        .map(|attachment| {
            format!(
                "--- 补充日志文件: {} ---\n{}",
                attachment.summary.name, attachment.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn history_attachments(
    attachments: &[LoadedAiAttachment],
) -> Vec<crate::ai_history::AiHistoryAttachment> {
    attachments
        .iter()
        .map(|attachment| crate::ai_history::AiHistoryAttachment {
            name: attachment.summary.name.clone(),
            content: attachment.content.clone(),
            char_count: attachment.summary.char_count,
        })
        .collect()
}

fn conversation_context(messages: &[crate::ai_history::AiHistoryMessage]) -> String {
    let mut recent = messages.iter().rev().take(12).collect::<Vec<_>>();
    recent.reverse();
    recent
        .into_iter()
        .map(|message| {
            let attachments = message
                .attachments
                .iter()
                .map(|attachment| {
                    format!(
                        "--- 历史补充日志文件: {} ---\n{}",
                        attachment.name, attachment.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            if attachments.is_empty() {
                format!("{}: {}", message.role, message.content)
            } else {
                format!("{}: {}\n\n{}", message.role, message.content, attachments)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn provider_protocol_label(protocol: AiProtocol) -> &'static str {
    match protocol {
        AiProtocol::ChatCompletions => "chatCompletions",
        AiProtocol::Responses => "responses",
    }
}

fn validate_history_target(
    record: &crate::ai_history::AiHistoryRecord,
    provider: &StoredAiProvider,
) -> Result<(), String> {
    if record.provider_id != provider.id
        || record.protocol != provider_protocol_label(provider.protocol)
        || record.model != provider.model
        || record.endpoint_fingerprint != provider.base_url
    {
        return Err("AI 供应商目标已变化，请重新选择日志并确认后再分析".to_string());
    }
    Ok(())
}

fn validate_accumulated_attachment_chars(
    selected_text: &str,
    messages: &[crate::ai_history::AiHistoryMessage],
    attachments: &[LoadedAiAttachment],
) -> Result<(), String> {
    let historical_chars = messages
        .iter()
        .flat_map(|message| message.attachments.iter())
        .map(|attachment| attachment.content.chars().count())
        .sum::<usize>();
    let current_chars = attachments
        .iter()
        .map(|attachment| attachment.content.chars().count())
        .sum::<usize>();
    if selected_text
        .chars()
        .count()
        .saturating_add(historical_chars)
        .saturating_add(current_chars)
        > MAX_ANALYSIS_CHARS
    {
        return Err(format!(
            "原始日志与会话附件合计超过 {MAX_ANALYSIS_CHARS} 个字符限制"
        ));
    }
    Ok(())
}

async fn load_ai_attachments_async(
    selected_text: String,
    attachment_paths: Vec<String>,
) -> Result<Vec<LoadedAiAttachment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        load_ai_attachments(&selected_text, &attachment_paths)
    })
    .await
    .map_err(|_| "读取附件的后台任务失败".to_string())?
}

fn provider_endpoint(provider: &StoredAiProvider) -> String {
    if provider.endpoint_mode == AiEndpointMode::Full {
        return provider.base_url.trim().to_string();
    }
    let suffix = match provider.protocol {
        AiProtocol::ChatCompletions => "chat/completions",
        AiProtocol::Responses => "responses",
    };
    format!("{}/{suffix}", provider.base_url.trim_end_matches('/'))
}

fn ai_request_builder(
    client: &reqwest::Client,
    provider: &StoredAiProvider,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let request = client
        .post(provider_endpoint(provider))
        .bearer_auth(api_key);
    if provider.protocol != AiProtocol::Responses {
        return request;
    }

    request
        .header(SESSION_ID_HEADER, uuid::Uuid::now_v7().to_string())
        .header(THREAD_ID_HEADER, uuid::Uuid::now_v7().to_string())
        .header(reqwest::header::ACCEPT, "text/event-stream")
}

async fn send_ai_request(
    provider: &StoredAiProvider,
    api_key: &str,
    instructions: &str,
    input: &str,
    timeout_seconds: u64,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|_| "Unable to create the AI request client".to_string())?;
    let request = ai_request_builder(&client, provider, api_key);
    let response = match provider.protocol {
        AiProtocol::ChatCompletions => {
            request
                .json(&ChatRequest {
                    model: &provider.model,
                    temperature: 0.1,
                    messages: [
                        ChatMessage {
                            role: "system",
                            content: instructions,
                        },
                        ChatMessage {
                            role: "user",
                            content: input,
                        },
                    ],
                })
                .send()
                .await
        }
        AiProtocol::Responses => {
            request
                .json(&ResponsesRequest {
                    model: &provider.model,
                    instructions,
                    input,
                    store: false,
                    stream: true,
                })
                .send()
                .await
        }
    }
    .map_err(|_| "AI provider request failed".to_string())?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let raw_body = response
        .bytes()
        .await
        .map_err(|_| "AI provider returned an invalid response".to_string())?;
    if !status.is_success() {
        return Err(format!("AI provider returned HTTP {}", status.as_u16()));
    }
    let body_size = raw_body.len();
    let raw_body = String::from_utf8_lossy(&raw_body);
    parse_ai_response(&raw_body).ok_or_else(|| {
        format!(
            "AI provider returned an invalid response (HTTP {}; Content-Type: {}; body: {} bytes)",
            status.as_u16(),
            content_type,
            body_size
        )
    })
}

fn parse_ai_response(raw_body: &str) -> Option<serde_json::Value> {
    let trimmed = raw_body.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(body) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(body);
    }

    let mut events = Vec::new();
    let mut deltas = Vec::new();
    for line in raw_body.lines() {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(delta) = stream_delta_text(&event) {
            deltas.push(delta);
        }
        events.push(event);
    }
    if !deltas.is_empty() {
        return Some(serde_json::json!({"output_text": deltas.join("")}));
    }
    if !events.is_empty() {
        let mut parts = Vec::new();
        for event in &events {
            if let Some(text) = response_content(AiProtocol::Responses, event)
                .or_else(|| response_content(AiProtocol::ChatCompletions, event))
            {
                parts.push(text);
            }
        }
        if !parts.is_empty() {
            return Some(serde_json::json!({"output_text": parts.join("\n")}));
        }
    }
    Some(serde_json::json!({"output_text": trimmed}))
}

fn stream_delta_text(event: &serde_json::Value) -> Option<String> {
    if let Some(delta) = event.get("delta").and_then(serde_json::Value::as_str) {
        return (!delta.is_empty()).then_some(delta.to_string());
    }
    event
        .get("choices")?
        .as_array()?
        .first()?
        .get("delta")?
        .get("content")?
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn chat_content(body: &serde_json::Value) -> Option<String> {
    body.get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

fn responses_content(body: &serde_json::Value) -> Option<String> {
    if let Some(output_text) = body.get("output_text").and_then(serde_json::Value::as_str) {
        return Some(output_text.to_string());
    }
    let content = body
        .get("output")?
        .as_array()?
        .iter()
        .filter_map(|item| item.get("content")?.as_array())
        .flatten()
        .filter_map(|content| content.get("text")?.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (!content.trim().is_empty()).then_some(content)
}

fn collect_compatible_text(value: &serde_json::Value, output: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) if !text.trim().is_empty() => output.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_compatible_text(item, output);
            }
        }
        serde_json::Value::Object(object) => {
            for key in ["text", "output_text", "content", "message", "answer"] {
                if let Some(value) = object.get(key) {
                    collect_compatible_text(value, output);
                }
            }
        }
        _ => {}
    }
}

fn response_content(protocol: AiProtocol, body: &serde_json::Value) -> Option<String> {
    let standard = match protocol {
        AiProtocol::ChatCompletions => chat_content(body).or_else(|| responses_content(body)),
        AiProtocol::Responses => responses_content(body).or_else(|| chat_content(body)),
    };
    if standard
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty())
    {
        return standard;
    }
    let mut parts = Vec::new();
    for key in [
        "output",
        "output_text",
        "response",
        "result",
        "data",
        "content",
        "answer",
    ] {
        if let Some(value) = body.get(key) {
            collect_compatible_text(value, &mut parts);
        }
    }
    let content = parts.join("\n");
    (!content.trim().is_empty()).then_some(content)
}

fn analysis_content(protocol: AiProtocol, body: &serde_json::Value) -> Option<String> {
    response_content(protocol, body).or_else(|| {
        let raw = serde_json::to_string_pretty(body).ok()?;
        (!matches!(body, serde_json::Value::Null) && raw != "{}" && raw != "[]").then_some(raw)
    })
}

fn key_entry(provider_id: &str) -> Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, provider_id)
}

pub fn save_api_key(provider_id: &str, api_key: &str) -> Result<(), keyring::Error> {
    key_entry(provider_id)?.set_password(api_key)
}

pub fn has_api_key(provider_id: &str) -> Result<bool, keyring::Error> {
    match key_entry(provider_id)?.get_password() {
        Ok(value) => Ok(!value.is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn read_api_key(provider_id: &str) -> Result<String, keyring::Error> {
    key_entry(provider_id)?.get_password()
}

pub fn delete_api_key(provider_id: &str) -> Result<(), keyring::Error> {
    match key_entry(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error),
    }
}

fn providers_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("ai-providers.json"))
        .map_err(|_| "AI provider settings directory is unavailable".to_string())
}

fn read_stored_providers(app: &AppHandle) -> Result<Vec<StoredAiProvider>, String> {
    let path = providers_path(app)?;
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(Vec::new());
    };
    serde_json::from_slice(&bytes).map_err(|_| "AI provider settings are invalid".to_string())
}

fn write_stored_providers(app: &AppHandle, providers: &[StoredAiProvider]) -> Result<(), String> {
    let path = providers_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| "Unable to create AI provider settings directory".to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(providers)
        .map_err(|_| "Unable to serialize AI provider settings".to_string())?;
    std::fs::write(path, bytes).map_err(|_| "Unable to save AI provider settings".to_string())
}

#[tauri::command]
pub fn list_ai_providers(app: AppHandle) -> Result<Vec<AiProviderConfig>, String> {
    read_stored_providers(&app)?
        .into_iter()
        .map(|provider| {
            let key_configured = has_api_key(&provider.id)
                .map_err(|_| "Unable to access the system credential store".to_string())?;
            Ok(AiProviderConfig {
                id: provider.id,
                name: provider.name,
                base_url: provider.base_url,
                model: provider.model,
                key_configured,
                protocol: provider.protocol,
                endpoint_mode: provider.endpoint_mode,
                allow_insecure_http: provider.allow_insecure_http,
            })
        })
        .collect()
}

#[tauri::command]
pub fn save_ai_provider(
    app: AppHandle,
    config: AiProviderConfig,
    api_key: Option<String>,
) -> Result<AiProviderConfig, String> {
    config.validate().map_err(|error| error.to_string())?;
    if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        save_api_key(&config.id, &api_key)
            .map_err(|_| "Unable to save the API key to the system credential store".to_string())?;
    }
    let mut providers = read_stored_providers(&app)?;
    let stored = StoredAiProvider {
        id: config.id.clone(),
        name: config.name.clone(),
        base_url: config.base_url.clone(),
        model: config.model.clone(),
        protocol: config.protocol,
        endpoint_mode: config.endpoint_mode,
        allow_insecure_http: config.allow_insecure_http,
    };
    if let Some(existing) = providers
        .iter_mut()
        .find(|provider| provider.id == stored.id)
    {
        *existing = stored;
    } else {
        providers.push(stored);
    }
    write_stored_providers(&app, &providers)?;
    Ok(AiProviderConfig {
        key_configured: has_api_key(&config.id)
            .map_err(|_| "Unable to access the system credential store".to_string())?,
        ..config
    })
}

#[tauri::command]
pub fn delete_ai_provider(app: AppHandle, provider_id: String) -> Result<(), String> {
    if !provider_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("Invalid AI provider id".to_string());
    }
    delete_api_key(&provider_id)
        .map_err(|_| "Unable to remove the API key from the system credential store".to_string())?;
    let mut providers = read_stored_providers(&app)?;
    providers.retain(|provider| provider.id != provider_id);
    write_stored_providers(&app, &providers)
}

#[tauri::command]
pub async fn test_ai_provider(app: AppHandle, provider_id: String) -> Result<(), String> {
    let provider = read_stored_providers(&app)?
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| "AI provider was not found".to_string())?;
    let api_key = read_api_key(&provider.id)
        .map_err(|_| "Unable to access the API key in the system credential store".to_string())?;
    if api_key.is_empty() {
        return Err("AI provider API key is not configured".to_string());
    }
    send_ai_request(
        &provider,
        &api_key,
        "This is a connection test. Follow the user request exactly.",
        "Reply with OK.",
        10,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn analyze_ai_log(
    app: AppHandle,
    provider_id: String,
    selected_text: String,
) -> Result<AiAnalysisResult, String> {
    validate_analysis_text(&selected_text)?;
    let provider = read_stored_providers(&app)?
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| "AI provider was not found".to_string())?;
    let api_key = read_api_key(&provider.id)
        .map_err(|_| "Unable to access the API key in the system credential store".to_string())?;
    if api_key.is_empty() {
        return Err("AI provider API key is not configured".to_string());
    }
    let body = send_ai_request(
        &provider,
        &api_key,
        INITIAL_ANALYSIS_INSTRUCTIONS,
        &selected_text,
        60,
    )
    .await?;
    let content = analysis_content(provider.protocol, &body)
        .ok_or_else(|| "AI provider returned no analysis content".to_string())?;
    Ok(AiAnalysisResult {
        provider_id,
        model: provider.model,
        content,
    })
}

#[tauri::command]
pub async fn inspect_ai_attachments(
    selected_text: String,
    attachment_paths: Vec<String>,
) -> Result<Vec<AiAttachmentSummary>, String> {
    validate_analysis_text(&selected_text)?;
    Ok(load_ai_attachments_async(selected_text, attachment_paths)
        .await?
        .into_iter()
        .map(|attachment| attachment.summary)
        .collect())
}

#[tauri::command]
pub async fn continue_ai_conversation(
    app: AppHandle,
    provider_id: String,
    selected_text: String,
    history: Vec<crate::ai_history::AiHistoryMessage>,
    question: String,
    attachment_paths: Vec<String>,
    history_update: Option<AiHistoryUpdate>,
) -> Result<AiAnalysisResult, String> {
    validate_analysis_text(&selected_text)?;
    if question.trim().is_empty() {
        return Err("请输入追问内容".into());
    }
    if question.chars().count() > 4_000 {
        return Err("追问内容超过 4000 个字符限制".into());
    }
    let provider = read_stored_providers(&app)?
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| "AI provider was not found".to_string())?;
    let api_key = read_api_key(&provider.id)
        .map_err(|_| "Unable to access the API key in the system credential store".to_string())?;
    let mut stored_history = history_update
        .as_ref()
        .map(|update| update.id.as_str())
        .map(|id| crate::ai_history::load_ai_history_record(&app, id))
        .transpose()?;
    if let Some(record) = stored_history.as_ref() {
        validate_history_target(record, &provider)?;
    }
    let attachments = load_ai_attachments_async(selected_text.clone(), attachment_paths).await?;
    if let Some(record) = stored_history.as_ref() {
        validate_accumulated_attachment_chars(&selected_text, &record.messages, &attachments)?;
    }
    let context = stored_history
        .as_ref()
        .map(|record| conversation_context(&record.messages))
        .unwrap_or_else(|| conversation_context(&history));
    let supplemental = attachment_context(&attachments);
    let input = if supplemental.is_empty() {
        format!("原始日志：\n{selected_text}\n\n已有对话：\n{context}\n\n用户追问：\n{question}")
    } else {
        format!("原始日志：\n{selected_text}\n\n补充日志：\n{supplemental}\n\n已有对话：\n{context}\n\n用户追问：\n{question}")
    };
    let body = send_ai_request(&provider, &api_key, FOLLOW_UP_INSTRUCTIONS, &input, 60).await?;
    let content = analysis_content(provider.protocol, &body)
        .ok_or_else(|| "AI provider returned no analysis content".to_string())?;
    if let Some(record) = stored_history.as_mut() {
        if let Some(update) = history_update {
            record.updated_at = update.updated_at;
        }
        record.messages.push(crate::ai_history::AiHistoryMessage {
            role: "user".to_string(),
            content: question,
            attachments: history_attachments(&attachments),
        });
        record.messages.push(crate::ai_history::AiHistoryMessage {
            role: "assistant".to_string(),
            content: content.clone(),
            attachments: Vec::new(),
        });
        crate::ai_history::save_ai_history_record(&app, record.clone())?;
    }
    Ok(AiAnalysisResult {
        provider_id,
        model: provider.model,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempAttachment {
        root: PathBuf,
        path: PathBuf,
    }

    impl TempAttachment {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let root = std::env::temp_dir().join(format!("logcrate-ai-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&root).expect("temporary attachment directory");
            let path = root.join(name);
            std::fs::write(&path, bytes).expect("temporary attachment contents");
            Self { root, path }
        }

        fn path_string(&self) -> String {
            self.path.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempAttachment {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn config(base_url: &str) -> AiProviderConfig {
        AiProviderConfig {
            id: "custom-provider".into(),
            name: "Custom".into(),
            base_url: base_url.into(),
            model: "model-1".into(),
            key_configured: false,
            protocol: AiProtocol::ChatCompletions,
            endpoint_mode: AiEndpointMode::Base,
            allow_insecure_http: false,
        }
    }

    fn stored(protocol: AiProtocol, endpoint_mode: AiEndpointMode) -> StoredAiProvider {
        StoredAiProvider {
            id: "test".into(),
            name: "Test".into(),
            base_url: "https://api.example.com/v1".into(),
            model: "model-1".into(),
            protocol,
            endpoint_mode,
            allow_insecure_http: false,
        }
    }

    #[test]
    fn accepts_https_and_localhost_http_endpoints() {
        assert!(config("https://api.example.com/v1").validate().is_ok());
        assert!(config("http://localhost:8080/v1").validate().is_ok());
        assert!(config("http://127.0.0.1/v1").validate().is_ok());
    }

    #[test]
    fn rejects_public_http_and_invalid_provider_ids() {
        assert_eq!(
            config("http://api.example.com/v1").validate(),
            Err(AiProviderConfigError::InvalidEndpoint)
        );
        let mut invalid = config("https://api.example.com/v1");
        invalid.id = "provider/key".into();
        assert_eq!(invalid.validate(), Err(AiProviderConfigError::InvalidId));

        let mut explicitly_allowed = config("http://api.internal.example/v1");
        explicitly_allowed.allow_insecure_http = true;
        assert!(explicitly_allowed.validate().is_ok());
    }

    #[test]
    fn analysis_text_is_bounded_and_must_not_be_blank() {
        assert!(validate_analysis_text("ERROR something").is_ok());
        assert!(validate_analysis_text(" \n\t ").is_err());
        assert!(validate_analysis_text(&"x".repeat(MAX_ANALYSIS_CHARS + 1)).is_err());
    }

    #[test]
    fn analysis_prompts_require_concise_event_sections_and_markdown() {
        assert!(INITIAL_ANALYSIS_INSTRUCTIONS.contains("按原日志出现顺序"));
        assert!(INITIAL_ANALYSIS_INSTRUCTIONS.contains("## N. 事件名称"));
        assert!(INITIAL_ANALYSIS_INSTRUCTIONS.contains("重复或同类日志必须合并"));
        assert!(INITIAL_ANALYSIS_INSTRUCTIONS.contains("不超过 3 句话"));
        assert!(FOLLOW_UP_INSTRUCTIONS.contains("不要重复完整报告"));
    }

    #[test]
    fn loads_bounded_text_attachments_and_builds_labeled_context() {
        let utf8 = TempAttachment::new("server.log", "ERROR 数据库超时".as_bytes());
        let (gb18030, _, _) = encoding_rs::GB18030.encode("WARN 连接重试");
        let legacy = TempAttachment::new("legacy.txt", &gb18030);
        let attachments =
            load_ai_attachments("original log", &[utf8.path_string(), legacy.path_string()])
                .expect("valid text attachments");

        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].summary.name, "server.log");
        assert_eq!(attachments[0].summary.char_count, 11);
        assert!(attachments[0].summary.path.ends_with("server.log"));
        assert_eq!(attachments[1].content, "WARN 连接重试");
        let context = attachment_context(&attachments);
        assert!(context.contains("--- 补充日志文件: server.log ---"));
        assert!(context.contains("ERROR 数据库超时"));
        assert!(context.contains("WARN 连接重试"));
    }

    #[test]
    fn rejects_attachment_count_size_type_duplicates_and_total_char_overflow() {
        let too_many = vec!["missing.log".to_string(); MAX_AI_ATTACHMENTS + 1];
        assert!(load_ai_attachments("base", &too_many)
            .expect_err("attachment count should be bounded")
            .contains("最多添加 5 个附件"));

        let archive = TempAttachment::new("bundle.zip", b"plain-looking bytes");
        assert!(load_ai_attachments("base", &[archive.path_string()])
            .expect_err("archives should be rejected")
            .contains("bundle.zip"));

        let binary = TempAttachment::new("dump.bin", b"prefix\0payload");
        assert!(load_ai_attachments("base", &[binary.path_string()])
            .expect_err("binary data should be rejected")
            .contains("dump.bin"));

        let oversized =
            TempAttachment::new("oversized.log", &vec![b'x'; MAX_AI_ATTACHMENT_BYTES + 1]);
        assert!(load_ai_attachments("base", &[oversized.path_string()])
            .expect_err("oversized files should be rejected")
            .contains("256 KiB"));

        let duplicate = TempAttachment::new("duplicate.log", b"same file");
        assert!(
            load_ai_attachments("base", &[duplicate.path_string(), duplicate.path_string()])
                .expect_err("duplicate paths should be rejected")
                .contains("重复添加")
        );

        let overflow = TempAttachment::new("overflow.log", b"xx");
        assert!(load_ai_attachments(
            &"x".repeat(MAX_ANALYSIS_CHARS - 1),
            &[overflow.path_string()]
        )
        .expect_err("combined context should be bounded")
        .contains("合计超过 120000 个字符"));
    }

    #[test]
    fn attachment_read_errors_do_not_expose_parent_directories() {
        let private_parent = std::env::temp_dir()
            .join("private-customer-name")
            .join("missing.log");
        let error = load_ai_attachments("base", &[private_parent.to_string_lossy().into_owned()])
            .expect_err("missing attachments should fail");
        assert!(error.contains("missing.log"));
        assert!(!error.contains("private-customer-name"));
    }

    #[test]
    fn restored_attachment_content_is_included_in_chronological_context() {
        let messages = vec![
            crate::ai_history::AiHistoryMessage {
                role: "user".into(),
                content: "compare".into(),
                attachments: vec![crate::ai_history::AiHistoryAttachment {
                    name: "context.log".into(),
                    content: "ERROR restored attachment".into(),
                    char_count: 25,
                }],
            },
            crate::ai_history::AiHistoryMessage {
                role: "assistant".into(),
                content: "first answer".into(),
                attachments: Vec::new(),
            },
        ];
        let context = conversation_context(&messages);
        assert!(context.starts_with("user: compare"));
        assert!(context.contains("--- 历史补充日志文件: context.log ---"));
        assert!(context.contains("ERROR restored attachment"));
        assert!(context.ends_with("assistant: first answer"));
    }

    #[test]
    fn restored_history_requires_the_same_provider_target() {
        let provider = stored(AiProtocol::ChatCompletions, AiEndpointMode::Base);
        let mut record = crate::ai_history::AiHistoryRecord {
            id: "history".into(),
            title: "title".into(),
            created_at: "created".into(),
            updated_at: "updated".into(),
            provider_id: provider.id.clone(),
            protocol: "chatCompletions".into(),
            model: provider.model.clone(),
            endpoint_fingerprint: provider.base_url.clone(),
            selected_text: "log".into(),
            messages: Vec::new(),
        };
        assert!(validate_history_target(&record, &provider).is_ok());
        record.endpoint_fingerprint = "https://changed.example/v1".into();
        assert!(validate_history_target(&record, &provider).is_err());
    }

    #[test]
    fn restored_and_current_attachments_share_the_session_character_limit() {
        let current = TempAttachment::new("current.log", b"xx");
        let loaded = load_ai_attachments("base", &[current.path_string()]).expect("current");
        let messages = vec![crate::ai_history::AiHistoryMessage {
            role: "user".into(),
            content: "previous".into(),
            attachments: vec![crate::ai_history::AiHistoryAttachment {
                name: "previous.log".into(),
                content: "x".repeat(MAX_ANALYSIS_CHARS - 5),
                char_count: MAX_ANALYSIS_CHARS - 5,
            }],
        }];
        assert!(validate_accumulated_attachment_chars("base", &messages, &loaded).is_err());
    }

    #[test]
    fn resolves_protocol_paths_and_preserves_full_urls() {
        assert_eq!(
            provider_endpoint(&stored(AiProtocol::ChatCompletions, AiEndpointMode::Base)),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            provider_endpoint(&stored(AiProtocol::Responses, AiEndpointMode::Base)),
            "https://api.example.com/v1/responses"
        );
        assert_eq!(
            provider_endpoint(&stored(AiProtocol::Responses, AiEndpointMode::Full)),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn extracts_chat_and_responses_content() {
        let chat = serde_json::json!({"choices": [{"message": {"content": "chat result"}}]});
        assert_eq!(
            response_content(AiProtocol::ChatCompletions, &chat).as_deref(),
            Some("chat result")
        );
        let responses = serde_json::json!({
            "output": [{"content": [{"type": "output_text", "text": "first"}, {"type": "output_text", "text": "second"}]}]
        });
        assert_eq!(
            response_content(AiProtocol::Responses, &responses).as_deref(),
            Some("first\nsecond")
        );
        let request = serde_json::to_value(ResponsesRequest {
            model: "model-1",
            instructions: "analyze",
            input: "selected log",
            store: false,
            stream: true,
        })
        .expect("Responses request should serialize");
        assert_eq!(
            request.get("store").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            request.get("stream").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        let compatible = serde_json::json!({
            "response": {"message": {"content": "company gateway result"}}
        });
        assert_eq!(
            response_content(AiProtocol::Responses, &compatible).as_deref(),
            Some("company gateway result")
        );
        let unexpected = serde_json::json!({"unexpected": 42});
        assert_eq!(
            analysis_content(AiProtocol::Responses, &unexpected).as_deref(),
            Some("{\n  \"unexpected\": 42\n}")
        );
    }

    #[test]
    fn accepts_plain_text_and_sse_responses() {
        let plain = parse_ai_response("company gateway plain text").expect("plain text body");
        assert_eq!(
            analysis_content(AiProtocol::Responses, &plain).as_deref(),
            Some("company gateway plain text")
        );

        let sse = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"first\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" second\"}\n\n",
            "data: [DONE]\n"
        );
        let body = parse_ai_response(sse).expect("SSE body");
        assert_eq!(
            analysis_content(AiProtocol::Responses, &body).as_deref(),
            Some("first second")
        );

        let chat_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"chat\"}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" result\"}}]}\n",
            "data: [DONE]\n"
        );
        let body = parse_ai_response(chat_sse).expect("Chat SSE body");
        assert_eq!(
            analysis_content(AiProtocol::ChatCompletions, &body).as_deref(),
            Some("chat result")
        );
    }

    #[test]
    fn responses_requests_include_codex_session_headers_only() {
        let client = reqwest::Client::new();
        let responses = stored(AiProtocol::Responses, AiEndpointMode::Base);
        let request = ai_request_builder(&client, &responses, "test-key")
            .build()
            .expect("Responses request should build");
        let session_id = request
            .headers()
            .get(SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("Responses request should contain session-id");
        let thread_id = request
            .headers()
            .get(THREAD_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("Responses request should contain thread-id");
        assert_eq!(
            uuid::Uuid::parse_str(session_id)
                .expect("session-id should be a UUID")
                .get_version_num(),
            7
        );
        assert_eq!(
            uuid::Uuid::parse_str(thread_id)
                .expect("thread-id should be a UUID")
                .get_version_num(),
            7
        );
        assert_ne!(session_id, thread_id);

        let chat = stored(AiProtocol::ChatCompletions, AiEndpointMode::Base);
        let request = ai_request_builder(&client, &chat, "test-key")
            .build()
            .expect("Chat Completions request should build");
        assert!(!request.headers().contains_key(SESSION_ID_HEADER));
        assert!(!request.headers().contains_key(THREAD_ID_HEADER));
    }

    #[test]
    fn old_stored_providers_receive_safe_protocol_defaults() {
        let provider: StoredAiProvider = serde_json::from_value(serde_json::json!({
            "id": "legacy",
            "name": "Legacy",
            "baseUrl": "https://api.example.com/v1",
            "model": "legacy-model"
        }))
        .expect("legacy provider should deserialize");
        assert_eq!(provider.protocol, AiProtocol::ChatCompletions);
        assert_eq!(provider.endpoint_mode, AiEndpointMode::Base);
        assert!(!provider.allow_insecure_http);
    }
}
