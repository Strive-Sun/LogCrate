//! Dedicated bounded DOCX preview sessions and lazy image reads.

use crate::archive::docx::{
    DocxBlock, DocxDocument, DocxImageBlock, DocxImageStatus, DocxParagraph, DocxParagraphRole,
    DocxTableBlock,
};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_DOCX_SESSIONS: usize = 5;
const MAX_PAGE_BLOCKS: u64 = 100;
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
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DocxPreviewBlock {
    Paragraph {
        index: u64,
        text: String,
        role: DocxParagraphRole,
        list_marker: Option<String>,
        list_level: Option<u8>,
    },
    Table {
        index: u64,
        rows: Vec<DocxPreviewTableRow>,
        column_count: u16,
        continuation: bool,
        search_text: String,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxPreviewTableRow {
    pub cells: Vec<DocxPreviewTableCell>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxPreviewTableCell {
    pub paragraphs: Vec<DocxPreviewParagraph>,
    pub col_span: u16,
    pub row_span: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxPreviewParagraph {
    pub text: String,
    pub role: DocxParagraphRole,
    pub list_marker: Option<String>,
    pub list_level: Option<u8>,
}

#[derive(Debug)]
enum StoredBlock {
    Paragraph(StoredParagraph),
    Table(StoredTable),
    Image(DocxImageBlock),
}

#[derive(Debug)]
struct StoredParagraph {
    offset: u64,
    length: u64,
    role: DocxParagraphRole,
    list_marker: Option<String>,
    list_level: Option<u8>,
}

#[derive(Debug)]
struct StoredTable {
    rows: Vec<StoredTableRow>,
    column_count: u16,
    continuation: bool,
}

#[derive(Debug)]
struct StoredTableRow {
    cells: Vec<StoredTableCell>,
}

#[derive(Debug)]
struct StoredTableCell {
    paragraphs: Vec<StoredParagraph>,
    col_span: u16,
    row_span: u16,
}

fn store_paragraph(
    writer: &mut BufWriter<File>,
    offset: &mut u64,
    paragraph: DocxParagraph,
) -> anyhow::Result<StoredParagraph> {
    writer.write_all(paragraph.text.as_bytes())?;
    let length = paragraph.text.len() as u64;
    let stored = StoredParagraph {
        offset: *offset,
        length,
        role: paragraph.role,
        list_marker: paragraph.list_marker,
        list_level: paragraph.list_level,
    };
    *offset = (*offset).saturating_add(length);
    Ok(stored)
}

fn store_table(
    writer: &mut BufWriter<File>,
    offset: &mut u64,
    table: DocxTableBlock,
) -> anyhow::Result<StoredTable> {
    let rows = table
        .rows
        .into_iter()
        .map(|row| {
            let cells = row
                .cells
                .into_iter()
                .map(|cell| {
                    let paragraphs = cell
                        .paragraphs
                        .into_iter()
                        .map(|paragraph| store_paragraph(writer, offset, paragraph))
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    Ok(StoredTableCell {
                        paragraphs,
                        col_span: cell.col_span,
                        row_span: cell.row_span,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(StoredTableRow { cells })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(StoredTable {
        rows,
        column_count: table.column_count,
        continuation: table.continuation,
    })
}

fn read_cached_text(cache: &mut File, offset: u64, length: u64) -> anyhow::Result<String> {
    let mut bytes = vec![0; length as usize];
    cache.seek(SeekFrom::Start(offset))?;
    cache.read_exact(&mut bytes)?;
    Ok(String::from_utf8(bytes)?)
}

fn read_stored_paragraph(
    cache: &mut File,
    paragraph: &StoredParagraph,
) -> anyhow::Result<DocxPreviewParagraph> {
    Ok(DocxPreviewParagraph {
        text: read_cached_text(cache, paragraph.offset, paragraph.length)?,
        role: paragraph.role,
        list_marker: paragraph.list_marker.clone(),
        list_level: paragraph.list_level,
    })
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
                    DocxBlock::Paragraph(paragraph) => blocks.push(StoredBlock::Paragraph(
                        store_paragraph(&mut writer, &mut offset, paragraph)?,
                    )),
                    DocxBlock::Table(table) => blocks.push(StoredBlock::Table(store_table(
                        &mut writer,
                        &mut offset,
                        table,
                    )?)),
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
                StoredBlock::Paragraph(paragraph) => {
                    result.push(DocxPreviewBlock::Paragraph {
                        index,
                        text: read_cached_text(&mut cache, paragraph.offset, paragraph.length)?,
                        role: paragraph.role,
                        list_marker: paragraph.list_marker.clone(),
                        list_level: paragraph.list_level,
                    });
                }
                StoredBlock::Table(table) => {
                    let rows = table
                        .rows
                        .iter()
                        .map(|row| {
                            let cells = row
                                .cells
                                .iter()
                                .map(|cell| {
                                    let paragraphs = cell
                                        .paragraphs
                                        .iter()
                                        .map(|paragraph| {
                                            read_stored_paragraph(&mut cache, paragraph)
                                        })
                                        .collect::<anyhow::Result<Vec<_>>>()?;
                                    Ok(DocxPreviewTableCell {
                                        paragraphs,
                                        col_span: cell.col_span,
                                        row_span: cell.row_span,
                                    })
                                })
                                .collect::<anyhow::Result<Vec<_>>>()?;
                            Ok(DocxPreviewTableRow { cells })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    result.push(DocxPreviewBlock::Table {
                        index,
                        search_text: rows
                            .iter()
                            .map(|row| {
                                row.cells
                                    .iter()
                                    .map(|cell| {
                                        cell.paragraphs
                                            .iter()
                                            .map(|paragraph| paragraph.text.as_str())
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\t")
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        rows,
                        column_count: table.column_count,
                        continuation: table.continuation,
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
            ("word/document.xml", br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:t>Hello</w:t><w:drawing><wp:inline><a:graphic><a:blip r:embed="img1"/></a:graphic></wp:inline></w:drawing></w:r></w:p><w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Cell A</w:t></w:r></w:p><w:p><w:r><w:t>Cell B</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#.to_vec()),
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
        assert!(matches!(&blocks[0], DocxPreviewBlock::Paragraph { text, .. } if text == "Hello"));
        let table = blocks
            .iter()
            .find_map(|block| match block {
                DocxPreviewBlock::Table {
                    rows, search_text, ..
                } => Some((rows, search_text)),
                _ => None,
            })
            .expect("structured table block");
        assert_eq!(table.0[0].cells[0].col_span, 2);
        assert_eq!(table.0[0].cells[0].paragraphs.len(), 2);
        assert_eq!(table.1, "Cell A\nCell B");
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
    fn preview_image_block_serializes_with_frontend_field_names() {
        let value = serde_json::to_value(DocxPreviewBlock::Image {
            index: 2,
            image_id: "opaque-image".to_string(),
            mime_type: Some("image/png".to_string()),
            alt_text: Some("Screenshot".to_string()),
            width_emu: Some(914_400),
            height_emu: Some(457_200),
            status: DocxImageStatus::Supported,
        })
        .unwrap();

        assert_eq!(value["kind"], "image");
        assert_eq!(value["imageId"], "opaque-image");
        assert_eq!(value["mimeType"], "image/png");
        assert_eq!(value["altText"], "Screenshot");
        assert_eq!(value["widthEmu"], 914_400);
        assert_eq!(value["heightEmu"], 457_200);
        assert!(value.get("image_id").is_none());
    }

    #[test]
    fn structured_blocks_serialize_with_frontend_field_names() {
        let paragraph = serde_json::to_value(DocxPreviewBlock::Paragraph {
            index: 0,
            text: "Heading".to_string(),
            role: DocxParagraphRole::Heading1,
            list_marker: None,
            list_level: None,
        })
        .unwrap();
        assert_eq!(paragraph["kind"], "paragraph");
        assert_eq!(paragraph["role"], "heading1");

        let table = serde_json::to_value(DocxPreviewBlock::Table {
            index: 1,
            rows: vec![DocxPreviewTableRow {
                cells: vec![DocxPreviewTableCell {
                    paragraphs: vec![DocxPreviewParagraph {
                        text: "Cell".to_string(),
                        role: DocxParagraphRole::Normal,
                        list_marker: None,
                        list_level: None,
                    }],
                    col_span: 2,
                    row_span: 3,
                }],
            }],
            column_count: 2,
            continuation: false,
            search_text: "Cell".to_string(),
        })
        .unwrap();
        assert_eq!(table["kind"], "table");
        assert_eq!(table["columnCount"], 2);
        assert_eq!(table["searchText"], "Cell");
        assert_eq!(table["rows"][0]["cells"][0]["colSpan"], 2);
        assert_eq!(table["rows"][0]["cells"][0]["rowSpan"], 3);
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
