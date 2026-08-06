use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use keyring::Entry;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

const SERVICE: &str = "logcrate.ai-history";
const KEY_USER: &str = "master-key-v1";
const MAX_RECORDS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryRecord {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider_id: String,
    pub protocol: String,
    pub model: String,
    pub endpoint_fingerprint: String,
    pub selected_text: String,
    pub messages: Vec<AiHistoryMessage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryAttachment {
    pub name: String,
    #[serde(default)]
    pub content: String,
    pub char_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AiHistoryAttachment>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistorySummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryRecordView {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider_id: String,
    pub protocol: String,
    pub model: String,
    pub endpoint_fingerprint: String,
    pub selected_text: String,
    pub messages: Vec<AiHistoryMessageView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryMessageView {
    pub role: String,
    pub content: String,
    pub attachments: Vec<AiHistoryAttachmentView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryAttachmentView {
    pub name: String,
    pub char_count: usize,
}

impl From<AiHistoryRecord> for AiHistoryRecordView {
    fn from(record: AiHistoryRecord) -> Self {
        Self {
            id: record.id,
            title: record.title,
            created_at: record.created_at,
            updated_at: record.updated_at,
            provider_id: record.provider_id,
            protocol: record.protocol,
            model: record.model,
            endpoint_fingerprint: record.endpoint_fingerprint,
            selected_text: record.selected_text,
            messages: record
                .messages
                .into_iter()
                .map(|message| AiHistoryMessageView {
                    role: message.role,
                    content: message.content,
                    attachments: message
                        .attachments
                        .into_iter()
                        .map(|attachment| AiHistoryAttachmentView {
                            name: attachment.name,
                            char_count: attachment.char_count,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u8,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "应用数据目录不可用".to_string())?
        .join("ai-history.enc"))
}

fn key() -> Result<[u8; 32], String> {
    let entry = Entry::new(SERVICE, KEY_USER).map_err(|_| "系统密钥链不可用".to_string())?;
    match entry.get_password() {
        Ok(value) => {
            if value.len() != 64 {
                return Err("历史加密密钥无效".to_string());
            }
            let mut bytes = [0u8; 32];
            for (index, slot) in bytes.iter_mut().enumerate() {
                *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                    .map_err(|_| "历史加密密钥无效".to_string())?;
            }
            Ok(bytes)
        }
        Err(keyring::Error::NoEntry) => {
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            let encoded = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            entry
                .set_password(&encoded)
                .map_err(|_| "无法写入系统密钥链".to_string())?;
            Ok(bytes)
        }
        Err(_) => Err("无法读取系统密钥链".to_string()),
    }
}

fn encrypt_records(
    records: &[AiHistoryRecord],
    encryption_key: &[u8; 32],
    nonce: &[u8; 12],
) -> Result<Vec<u8>, String> {
    let plain = serde_json::to_vec(records).map_err(|_| "无法序列化 AI 历史记录".to_string())?;
    let cipher =
        Aes256Gcm::new_from_slice(encryption_key).map_err(|_| "历史加密初始化失败".to_string())?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(nonce), plain.as_ref())
        .map_err(|_| "AI 历史记录加密失败".to_string())?;
    serde_json::to_vec(&Envelope {
        version: 1,
        nonce: nonce.to_vec(),
        ciphertext,
    })
    .map_err(|_| "无法序列化 AI 历史记录".to_string())
}

fn decrypt_records(
    bytes: &[u8],
    encryption_key: &[u8; 32],
) -> Result<Vec<AiHistoryRecord>, String> {
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| "AI 历史记录格式无效".to_string())?;
    if envelope.version != 1 || envelope.nonce.len() != 12 {
        return Err("AI 历史记录版本无效".into());
    }
    let cipher =
        Aes256Gcm::new_from_slice(encryption_key).map_err(|_| "历史加密初始化失败".to_string())?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            envelope.ciphertext.as_ref(),
        )
        .map_err(|_| "AI 历史记录解密失败".to_string())?;
    serde_json::from_slice(&plain).map_err(|_| "AI 历史记录内容无效".to_string())
}

fn encrypt_records_with_key<F>(
    records: &[AiHistoryRecord],
    nonce: &[u8; 12],
    key_provider: F,
) -> Result<Vec<u8>, String>
where
    F: FnOnce() -> Result<[u8; 32], String>,
{
    let encryption_key = key_provider()?;
    encrypt_records(records, &encryption_key, nonce)
}

fn decrypt_records_with_key<F>(
    bytes: &[u8],
    key_provider: F,
) -> Result<Vec<AiHistoryRecord>, String>
where
    F: FnOnce() -> Result<[u8; 32], String>,
{
    let encryption_key = key_provider()?;
    decrypt_records(bytes, &encryption_key)
}

fn upsert_record(
    records: &mut Vec<AiHistoryRecord>,
    record: AiHistoryRecord,
) -> Result<(), String> {
    if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
        *existing = record;
        return Ok(());
    }
    if records.len() >= MAX_RECORDS {
        return Err("AI 历史记录已达到 100 条上限，请先删除旧记录".into());
    }
    records.insert(0, record);
    Ok(())
}

fn delete_record(records: &mut Vec<AiHistoryRecord>, id: &str) {
    records.retain(|record| record.id != id);
}

fn clear_records(records: &mut Vec<AiHistoryRecord>) {
    records.clear();
}

fn read(app: &AppHandle) -> Result<Vec<AiHistoryRecord>, String> {
    let file = path(app)?;
    if !file.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(file).map_err(|_| "无法读取 AI 历史记录".to_string())?;
    decrypt_records_with_key(&bytes, key)
}

fn write(app: &AppHandle, records: &[AiHistoryRecord]) -> Result<(), String> {
    if records.len() > MAX_RECORDS {
        return Err("AI 历史记录已达到 100 条上限，请先删除旧记录".into());
    }
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let bytes = encrypt_records_with_key(records, &nonce, key)?;
    let file = path(app)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|_| "无法创建历史记录目录".to_string())?;
    }
    fs::write(file, bytes).map_err(|_| "无法保存 AI 历史记录".to_string())
}

pub(crate) fn load_ai_history_record(app: &AppHandle, id: &str) -> Result<AiHistoryRecord, String> {
    read(app)?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| "AI 历史记录不存在".into())
}

pub(crate) fn save_ai_history_record(
    app: &AppHandle,
    record: AiHistoryRecord,
) -> Result<(), String> {
    let mut records = read(app)?;
    upsert_record(&mut records, record)?;
    write(app, &records)
}

#[tauri::command]
pub fn list_ai_history(app: AppHandle) -> Result<Vec<AiHistorySummary>, String> {
    Ok(read(&app)?
        .into_iter()
        .map(|record| AiHistorySummary {
            id: record.id,
            title: record.title,
            created_at: record.created_at,
            updated_at: record.updated_at,
            provider_id: record.provider_id,
            model: record.model,
        })
        .collect())
}

#[tauri::command]
pub fn load_ai_history(app: AppHandle, id: String) -> Result<AiHistoryRecordView, String> {
    load_ai_history_record(&app, &id).map(Into::into)
}

#[tauri::command]
pub fn save_ai_history(app: AppHandle, record: AiHistoryRecord) -> Result<(), String> {
    save_ai_history_record(&app, record)
}

#[tauri::command]
pub fn delete_ai_history(app: AppHandle, id: String) -> Result<(), String> {
    let mut records = read(&app)?;
    delete_record(&mut records, &id);
    write(&app, &records)
}

#[tauri::command]
pub fn clear_ai_history(app: AppHandle) -> Result<(), String> {
    let mut records = Vec::new();
    clear_records(&mut records);
    write(&app, &records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_with_attachment() -> AiHistoryRecord {
        AiHistoryRecord {
            id: "history-1".into(),
            title: "Attachment analysis".into(),
            created_at: "2026-08-06T00:00:00Z".into(),
            updated_at: "2026-08-06T00:00:01Z".into(),
            provider_id: "provider".into(),
            protocol: "chatCompletions".into(),
            model: "model".into(),
            endpoint_fingerprint: "https://example.test/v1".into(),
            selected_text: "original secret log".into(),
            messages: vec![AiHistoryMessage {
                role: "user".into(),
                content: "compare the logs".into(),
                attachments: vec![AiHistoryAttachment {
                    name: "context.log".into(),
                    content: "attachment secret payload".into(),
                    char_count: 25,
                }],
            }],
        }
    }

    #[test]
    fn encrypted_history_round_trips_attachment_content_without_plaintext_leakage() {
        let records = vec![record_with_attachment()];
        let encryption_key = [7u8; 32];
        let nonce = [9u8; 12];
        let encrypted = encrypt_records(&records, &encryption_key, &nonce).expect("encrypt");
        let serialized = String::from_utf8(encrypted.clone()).expect("JSON envelope");
        assert!(!serialized.contains("attachment secret payload"));
        assert!(!serialized.contains("original secret log"));

        let restored = decrypt_records(&encrypted, &encryption_key).expect("decrypt");
        assert_eq!(restored[0].messages[0].attachments[0].name, "context.log");
        assert_eq!(
            restored[0].messages[0].attachments[0].content,
            "attachment secret payload"
        );
    }

    #[test]
    fn legacy_messages_without_attachments_remain_readable() {
        let legacy = serde_json::json!({
            "role": "assistant",
            "content": "legacy response"
        });
        let message: AiHistoryMessage =
            serde_json::from_value(legacy).expect("legacy message should deserialize");
        assert!(message.attachments.is_empty());
    }

    #[test]
    fn restored_history_view_exposes_attachment_metadata_without_content() {
        let view = AiHistoryRecordView::from(record_with_attachment());
        let serialized = serde_json::to_string(&view).expect("serialize history view");
        assert!(serialized.contains("context.log"));
        assert!(serialized.contains("charCount"));
        assert!(!serialized.contains("attachment secret payload"));
    }

    #[test]
    fn history_capacity_allows_one_hundred_updates_and_rejects_new_records_beyond_it() {
        let template = record_with_attachment();
        let mut records = (0..MAX_RECORDS)
            .map(|index| {
                let mut record = template.clone();
                record.id = format!("history-{index}");
                record
            })
            .collect::<Vec<_>>();

        let mut existing = template.clone();
        existing.id = "history-42".into();
        existing.title = "updated".into();
        upsert_record(&mut records, existing).expect("existing records remain updatable");
        assert_eq!(records.len(), MAX_RECORDS);
        assert_eq!(records[42].title, "updated");

        let mut overflow = template;
        overflow.id = "history-overflow".into();
        assert!(upsert_record(&mut records, overflow)
            .unwrap_err()
            .contains("100 条上限"));
        assert_eq!(records.len(), MAX_RECORDS);
        assert!(!records.iter().any(|record| record.id == "history-overflow"));
    }

    #[test]
    fn history_delete_and_clear_mutations_remove_only_the_requested_records() {
        let template = record_with_attachment();
        let mut records = ["first", "second", "third"]
            .into_iter()
            .map(|id| {
                let mut record = template.clone();
                record.id = id.into();
                record
            })
            .collect::<Vec<_>>();

        delete_record(&mut records, "second");
        assert_eq!(
            records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "third"]
        );
        delete_record(&mut records, "missing");
        assert_eq!(records.len(), 2);

        clear_records(&mut records);
        assert!(records.is_empty());
    }

    #[test]
    fn key_provider_failures_never_produce_or_restore_plaintext_history() {
        let records = vec![record_with_attachment()];
        let nonce = [3u8; 12];
        let key_error = || Err("系统密钥链不可用".to_string());
        assert_eq!(
            encrypt_records_with_key(&records, &nonce, key_error).unwrap_err(),
            "系统密钥链不可用"
        );

        let encrypted =
            encrypt_records_with_key(&records, &nonce, || Ok([4u8; 32])).expect("encrypt");
        assert!(!String::from_utf8_lossy(&encrypted).contains("original secret log"));
        assert_eq!(
            decrypt_records_with_key(&encrypted, || Err("无法读取系统密钥链".into())).unwrap_err(),
            "无法读取系统密钥链"
        );
    }

    #[test]
    fn damaged_or_wrong_key_ciphertext_fails_authentication_without_plaintext_fallback() {
        let records = vec![record_with_attachment()];
        let encryption_key = [5u8; 32];
        let mut envelope: Envelope = serde_json::from_slice(
            &encrypt_records(&records, &encryption_key, &[6u8; 12]).expect("encrypt"),
        )
        .expect("envelope");
        envelope.ciphertext[0] ^= 0x80;
        let damaged = serde_json::to_vec(&envelope).expect("damaged envelope");

        assert!(decrypt_records(&damaged, &encryption_key)
            .unwrap_err()
            .contains("解密失败"));
        let intact = encrypt_records(&records, &encryption_key, &[7u8; 12]).expect("encrypt");
        assert!(decrypt_records(&intact, &[8u8; 32])
            .unwrap_err()
            .contains("解密失败"));
        assert!(!String::from_utf8_lossy(&damaged).contains("attachment secret payload"));
    }
}
