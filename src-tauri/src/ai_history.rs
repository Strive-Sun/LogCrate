use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryMessage {
    pub role: String,
    pub content: String,
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

#[derive(Serialize, Deserialize)]
struct Envelope { version: u8, nonce: Vec<u8>, ciphertext: Vec<u8> }

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|_| "应用数据目录不可用".to_string())?.join("ai-history.enc"))
}

fn key() -> Result<[u8; 32], String> {
    let entry = Entry::new(SERVICE, KEY_USER).map_err(|_| "系统密钥链不可用".to_string())?;
    match entry.get_password() {
        Ok(value) => {
            if value.len() != 64 { return Err("历史加密密钥无效".to_string()); }
            let mut bytes = [0u8; 32];
            for (index, slot) in bytes.iter_mut().enumerate() {
                *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| "历史加密密钥无效".to_string())?;
            }
            Ok(bytes)
        }
        Err(keyring::Error::NoEntry) => {
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            let encoded = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            entry.set_password(&encoded).map_err(|_| "无法写入系统密钥链".to_string())?;
            Ok(bytes)
        }
        Err(_) => Err("无法读取系统密钥链".to_string()),
    }
}

fn read(app: &AppHandle) -> Result<Vec<AiHistoryRecord>, String> {
    let file = path(app)?;
    if !file.exists() { return Ok(Vec::new()); }
    let bytes = fs::read(file).map_err(|_| "无法读取 AI 历史记录".to_string())?;
    let env: Envelope = serde_json::from_slice(&bytes).map_err(|_| "AI 历史记录格式无效".to_string())?;
    if env.version != 1 || env.nonce.len() != 12 { return Err("AI 历史记录版本无效".into()); }
    let cipher = Aes256Gcm::new_from_slice(&key()?).map_err(|_| "历史加密初始化失败".to_string())?;
    let plain = cipher.decrypt(Nonce::from_slice(&env.nonce), env.ciphertext.as_ref()).map_err(|_| "AI 历史记录解密失败".to_string())?;
    serde_json::from_slice(&plain).map_err(|_| "AI 历史记录内容无效".to_string())
}

fn write(app: &AppHandle, records: &[AiHistoryRecord]) -> Result<(), String> {
    if records.len() > MAX_RECORDS { return Err("AI 历史记录已达到 100 条上限，请先删除旧记录".into()); }
    let plain = serde_json::to_vec(records).map_err(|_| "无法序列化 AI 历史记录".to_string())?;
    let cipher = Aes256Gcm::new_from_slice(&key()?).map_err(|_| "历史加密初始化失败".to_string())?;
    let mut nonce = [0u8; 12]; rand::thread_rng().fill_bytes(&mut nonce);
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plain.as_ref()).map_err(|_| "AI 历史记录加密失败".to_string())?;
    let bytes = serde_json::to_vec(&Envelope { version: 1, nonce: nonce.to_vec(), ciphertext }).map_err(|_| "无法序列化 AI 历史记录".to_string())?;
    let file = path(app)?; if let Some(parent) = file.parent() { fs::create_dir_all(parent).map_err(|_| "无法创建历史记录目录".to_string())?; }
    fs::write(file, bytes).map_err(|_| "无法保存 AI 历史记录".to_string())
}

#[tauri::command]
pub fn list_ai_history(app: AppHandle) -> Result<Vec<AiHistorySummary>, String> {
    Ok(read(&app)?.into_iter().map(|r| AiHistorySummary { id:r.id, title:r.title, created_at:r.created_at, updated_at:r.updated_at, provider_id:r.provider_id, model:r.model }).collect())
}

#[tauri::command]
pub fn load_ai_history(app: AppHandle, id: String) -> Result<AiHistoryRecord, String> {
    read(&app)?.into_iter().find(|r| r.id == id).ok_or_else(|| "AI 历史记录不存在".into())
}

#[tauri::command]
pub fn save_ai_history(app: AppHandle, record: AiHistoryRecord) -> Result<(), String> {
    let mut records = read(&app)?; if let Some(existing) = records.iter_mut().find(|r| r.id == record.id) { *existing = record; } else { records.insert(0, record); } write(&app, &records)
}

#[tauri::command]
pub fn delete_ai_history(app: AppHandle, id: String) -> Result<(), String> { let records = read(&app)?.into_iter().filter(|r| r.id != id).collect::<Vec<_>>(); write(&app, &records) }

#[tauri::command]
pub fn clear_ai_history(app: AppHandle) -> Result<(), String> { write(&app, &[]) }
