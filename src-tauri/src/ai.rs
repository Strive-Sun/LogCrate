//! Secure AI provider configuration primitives.
//!
//! Non-sensitive provider metadata is kept separate from the API key. Keys are
//! stored only in the platform credential store and are never serialized.

#![allow(dead_code)] // Provider commands consume this module in the next task.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
const SESSION_ID_HEADER: &str = "session-id";
const THREAD_ID_HEADER: &str = "thread-id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnalysisResult {
    pub provider_id: String,
    pub model: String,
    pub content: String,
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
                })
                .send()
                .await
        }
    }
    .map_err(|_| "AI provider request failed".to_string())?;
    let status = response.status();
    let raw_body = response
        .text()
        .await
        .map_err(|_| "AI provider returned an invalid response".to_string())?;
    if !status.is_success() {
        return Err(format!("AI provider returned HTTP {}", status.as_u16()));
    }
    parse_ai_response(&raw_body)
        .ok_or_else(|| "AI provider returned an invalid response".to_string())
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
    let system_prompt = "你是日志分析助手。请基于用户提供的日志，使用清晰的中文分段说明：1. 日志包含的主要信息；2. 警告；3. 错误；4. 可能原因；5. 建议。不要臆造日志中不存在的事实，对无法确定的内容明确标注不确定性。";
    let body = send_ai_request(&provider, &api_key, system_prompt, &selected_text, 60).await?;
    let content = analysis_content(provider.protocol, &body)
        .ok_or_else(|| "AI provider returned no analysis content".to_string())?;
    Ok(AiAnalysisResult {
        provider_id,
        model: provider.model,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        })
        .expect("Responses request should serialize");
        assert_eq!(
            request.get("store").and_then(serde_json::Value::as_bool),
            Some(false)
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
