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
    let request = client
        .post(provider_endpoint(provider))
        .bearer_auth(api_key);
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
                })
                .send()
                .await
        }
    }
    .map_err(|_| "AI provider request failed".to_string())?;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| "AI provider returned an invalid response".to_string())?;
    if !status.is_success() {
        return Err(format!("AI provider returned HTTP {}", status.as_u16()));
    }
    Ok(body)
}

fn response_content(protocol: AiProtocol, body: &serde_json::Value) -> Option<String> {
    let content = match protocol {
        AiProtocol::ChatCompletions => body
            .get("choices")?
            .as_array()?
            .first()?
            .get("message")?
            .get("content")?
            .as_str()?
            .to_string(),
        AiProtocol::Responses => {
            if let Some(output_text) = body.get("output_text").and_then(serde_json::Value::as_str) {
                output_text.to_string()
            } else {
                body.get("output")?
                    .as_array()?
                    .iter()
                    .filter_map(|item| item.get("content")?.as_array())
                    .flatten()
                    .filter_map(|content| content.get("text")?.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    };
    (!content.trim().is_empty()).then_some(content)
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
    let body = send_ai_request(
        &provider,
        &api_key,
        "This is a connection test. Follow the user request exactly.",
        "Reply with OK.",
        10,
    )
    .await?;
    response_content(provider.protocol, &body)
        .map(|_| ())
        .ok_or_else(|| "AI provider returned no compatible content".to_string())
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
    let content = response_content(provider.protocol, &body)
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
