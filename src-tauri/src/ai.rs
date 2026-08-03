//! Secure AI provider configuration primitives.
//!
//! Non-sensitive provider metadata is kept separate from the API key. Keys are
//! stored only in the platform credential store and are never serialized.

#![allow(dead_code)] // Provider commands consume this module in the next task.

use keyring::Entry;
use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "com.logcrate.ai-provider";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub key_configured: bool,
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
        if !is_allowed_endpoint(&self.base_url) {
            return Err(AiProviderConfigError::InvalidEndpoint);
        }
        Ok(())
    }
}

fn is_allowed_endpoint(value: &str) -> bool {
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
    matches!(host, "localhost" | "127.0.0.1" | "::1")
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
    }
}
