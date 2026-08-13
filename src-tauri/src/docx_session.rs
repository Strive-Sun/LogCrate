//! Dedicated bounded DOCX preview sessions and lazy image reads.

use crate::archive::docx::{DocxBlock, DocxDocument, DocxImageBlock, DocxImageStatus};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_DOCX_SESSIONS: usize = 5;
const MAX_PAGE_BLOCKS: u64 = 500;
static DOCX_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocxResult {
    pub session_id: String,
    pub source_path: String,
    pub title: String,
    pub block_count: u64,
    pub evicted_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DocxPreviewBlock {
    Text {
        index: u64,
        text: String,
    },
    Image {
        index: u64,
        image_id: String,
        mime_type: Option<String>,
        alt_text: Option<String>,
        width_emu: Option<u64>,
        height_emu: Option<u64>,
        status: DocxImageStatus,
    },
}

#[derive(Debug)]
enum StoredBlock {
    Text { offset: u64, length: u64 },
    Image(DocxImageBlock),
}

#[derive(Debug)]
struct DocxSession {
    document: DocxDocument,
    cache_path: PathBuf,
    blocks: Vec<StoredBlock>,
    images: HashMap<String, DocxImageBlock>,
}

impl Drop for DocxSession {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.cache_path);
    }
}

#[derive(Default)]
pub struct DocxSessionManager {
    sessions: Mutex<HashMap<String, Arc<DocxSession>>>,
    opening: Mutex<HashMap<String, Arc<AtomicBool>>>,
    lru: Mutex<Vec<String>>,
    cache_dir: Mutex<Option<PathBuf>>,
}

impl DocxSessionManager {
    pub fn set_cache_dir(&self, dir: PathBuf) {
        let _ = std::fs::create_dir_all(&dir);
        *self.cache_dir.lock().unwrap() = Some(dir);
    }

    fn cache_path(&self, session_id: &str) -> PathBuf {
        self.cache_dir
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("logcrate-{session_id}.docx-text"))
    }

    pub fn begin_open(&self, request_id: &str) -> anyhow::Result<Arc<AtomicBool>> {
        if request_id.is_empty() || request_id.len() > 128 {
            anyhow::bail!("DOCX 打开请求 ID 无效");
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let mut opening = self.opening.lock().unwrap();
        if opening
            .insert(request_id.to_string(), cancel.clone())
            .is_some()
        {
            anyhow::bail!("DOCX 打开请求 ID 重复");
        }
        Ok(cancel)
    }

    pub fn open(
        &self,
        path: &Path,
        request_id: &str,
        cancel: &AtomicBool,
    ) -> anyhow::Result<OpenDocxResult> {
        let result = (|| {
            if path.to_string_lossy().contains("::") {
                anyhow::bail!("DOCX 专用会话只接受单个磁盘文档路径");
            }
            let canonical = std::fs::canonicalize(path)?;
            if !canonical.is_file() {
                anyhow::bail!("DOCX 路径不是普通文件");
            }
            self.open_registered(&canonical, cancel)
        })();
        self.opening.lock().unwrap().remove(request_id);
        result
    }

    fn open_registered(
        &self,
        canonical: &Path,
        cancel: &AtomicBool,
    ) -> anyhow::Result<OpenDocxResult> {
        if cancel.load(Ordering::Acquire) {
            anyhow::bail!("DOCX 打开已取消");
        }
        let document = DocxDocument::open(canonical)?;
        let session_id = format!(
            "docx-{}-{}",
            std::process::id(),
            DOCX_SESSION_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let cache_path = self.cache_path(&session_id);
        let cache = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&cache_path)?;
        let mut writer = BufWriter::new(cache);
        let mut offset = 0u64;
        let mut blocks = Vec::new();
        let mut images = HashMap::new();
        let parsed = document.parse_blocks_until(
            |block| {
                match block {
                    DocxBlock::Text { text } => {
                        writer.write_all(text.as_bytes())?;
                        let length = text.len() as u64;
                        blocks.push(StoredBlock::Text { offset, length });
                        offset = offset.saturating_add(length);
                    }
                    DocxBlock::Image(image) => {
                        if image.status == DocxImageStatus::Supported {
                            images.insert(image.image_id.clone(), image.clone());
                        }
                        blocks.push(StoredBlock::Image(image));
                    }
                }
                Ok(())
            },
            || cancel.load(Ordering::Acquire),
        );
        if let Err(error) = parsed.and_then(|_| writer.flush().map_err(Into::into)) {
            drop(writer);
            let _ = std::fs::remove_file(&cache_path);
            return Err(error);
        }
        drop(writer);

        let block_count = blocks.len() as u64;
        let session = Arc::new(DocxSession {
            document,
            cache_path,
            blocks,
            images,
        });
        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), session);
        let evicted_session_ids = self.touch_lru(&session_id);
        Ok(OpenDocxResult {
            session_id,
            source_path: canonical.to_string_lossy().into_owned(),
            title: canonical
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("document.docx")
                .to_string(),
            block_count,
            evicted_session_ids,
        })
    }

    pub fn cancel_open(&self, request_id: &str) {
        if let Some(cancel) = self.opening.lock().unwrap().get(request_id) {
            cancel.store(true, Ordering::Release);
        }
    }

    fn session(&self, session_id: &str) -> anyhow::Result<Arc<DocxSession>> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("DOCX session not found: {session_id}"))?;
        let _ = self.touch_lru(session_id);
        Ok(session)
    }

    fn touch_lru(&self, session_id: &str) -> Vec<String> {
        let mut lru = self.lru.lock().unwrap();
        lru.retain(|current| current != session_id);
        lru.push(session_id.to_string());
        let mut evicted = Vec::new();
        while lru.len() > MAX_DOCX_SESSIONS {
            let session_id = lru.remove(0);
            if self.sessions.lock().unwrap().remove(&session_id).is_some() {
                evicted.push(session_id);
            }
        }
        evicted
    }

    pub fn read_blocks(
        &self,
        session_id: &str,
        start: u64,
        count: u64,
    ) -> anyhow::Result<Vec<DocxPreviewBlock>> {
        if count > MAX_PAGE_BLOCKS {
            anyhow::bail!("DOCX 分页请求超过 {MAX_PAGE_BLOCKS} 块上限");
        }
        let session = self.session(session_id)?;
        let end = start.saturating_add(count).min(session.blocks.len() as u64);
        if start >= end {
            return Ok(Vec::new());
        }
        let mut cache = File::open(&session.cache_path)?;
        let mut result = Vec::with_capacity((end - start) as usize);
        for index in start..end {
            match &session.blocks[index as usize] {
                StoredBlock::Text { offset, length } => {
                    let mut bytes = vec![0; *length as usize];
                    cache.seek(SeekFrom::Start(*offset))?;
                    cache.read_exact(&mut bytes)?;
                    result.push(DocxPreviewBlock::Text {
                        index,
                        text: String::from_utf8(bytes)?,
                    });
                }
                StoredBlock::Image(image) => result.push(DocxPreviewBlock::Image {
                    index,
                    image_id: image.image_id.clone(),
                    mime_type: image.mime_type.clone(),
                    alt_text: image.alt_text.clone(),
                    width_emu: image.width_emu,
                    height_emu: image.height_emu,
                    status: image.status,
                }),
            }
        }
        Ok(result)
    }

    pub fn read_image(&self, session_id: &str, image_id: &str) -> anyhow::Result<Vec<u8>> {
        let session = self.session(session_id)?;
        let image = session
            .images
            .get(image_id)
            .ok_or_else(|| anyhow::anyhow!("DOCX 图片 ID 无效"))?;
        let target = image
            .target_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("DOCX 图片路径缺失"))?;
        let mime = image
            .mime_type
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("DOCX 图片 MIME 缺失"))?;
        session.document.read_supported_image(target, mime)
    }

    pub fn close(&self, session_id: &str) {
        self.lru.lock().unwrap().retain(|id| id != session_id);
        self.sessions.lock().unwrap().remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zip::write::SimpleFileOptions;

    static SEQ: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "logcrate-docx-session-{}-{}.docx",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let file = File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let mut png = vec![0; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&2u32.to_be_bytes());
        png[20..24].copy_from_slice(&3u32.to_be_bytes());
        let parts: Vec<(&str, Vec<u8>)> = vec![
            ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec()),
            ("_rels/.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec()),
            ("word/_rels/document.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/p.png"/></Relationships>"#.to_vec()),
            ("word/document.xml", br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:t>Hello</w:t><w:drawing><wp:inline><a:graphic><a:blip r:embed="img1"/></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#.to_vec()),
            ("word/media/p.png", png),
        ];
        for (name, bytes) in parts {
            archive.start_file(name, options).unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    #[test]
    fn session_pages_text_and_reads_images_only_by_opaque_id() {
        let path = fixture();
        let cache = path.with_extension("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let manager = DocxSessionManager::default();
        manager.set_cache_dir(cache.clone());
        let cancel = manager.begin_open("request-1").unwrap();
        let opened = manager.open(&path, "request-1", &cancel).unwrap();
        let blocks = manager.read_blocks(&opened.session_id, 0, 20).unwrap();
        assert_eq!(blocks.len() as u64, opened.block_count);
        assert!(matches!(&blocks[0], DocxPreviewBlock::Text { text, .. } if text == "Hello"));
        let image_id = blocks
            .iter()
            .find_map(|block| match block {
                DocxPreviewBlock::Image { image_id, .. } => Some(image_id.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            manager
                .read_image(&opened.session_id, &image_id)
                .unwrap()
                .len(),
            24
        );
        assert!(manager
            .read_image(&opened.session_id, "word/media/p.png")
            .is_err());
        assert!(manager.read_blocks(&opened.session_id, 0, 501).is_err());
        manager.close(&opened.session_id);
        assert!(manager.read_blocks(&opened.session_id, 0, 1).is_err());
        assert!(!cache
            .join(format!("logcrate-{}.docx-text", opened.session_id))
            .exists());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(cache);
    }

    #[test]
    fn cancelled_open_publishes_no_session_and_leaves_no_cache() {
        let path = fixture();
        let cache = path.with_extension("cancel-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let manager = DocxSessionManager::default();
        manager.set_cache_dir(cache.clone());
        let cancel = manager.begin_open("cancelled-request").unwrap();
        manager.cancel_open("cancelled-request");
        let error = manager
            .open(&path, "cancelled-request", &cancel)
            .expect_err("cancelled open must fail");
        assert!(error.to_string().contains("取消"));
        assert!(manager.sessions.lock().unwrap().is_empty());
        assert!(manager.opening.lock().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(cache);
    }

    #[test]
    fn lru_eviction_releases_old_session_cache() {
        let path = fixture();
        let cache = path.with_extension("lru-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let manager = DocxSessionManager::default();
        manager.set_cache_dir(cache.clone());
        let mut opened = Vec::new();
        for index in 0..=MAX_DOCX_SESSIONS {
            let request_id = format!("lru-{index}");
            let cancel = manager.begin_open(&request_id).unwrap();
            opened.push(manager.open(&path, &request_id, &cancel).unwrap());
        }
        let first = &opened[0];
        assert_eq!(
            opened.last().unwrap().evicted_session_ids,
            vec![first.session_id.clone()]
        );
        assert!(manager.read_blocks(&first.session_id, 0, 1).is_err());
        assert!(!cache
            .join(format!("logcrate-{}.docx-text", first.session_id))
            .exists());
        drop(manager);
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(cache);
    }
}
