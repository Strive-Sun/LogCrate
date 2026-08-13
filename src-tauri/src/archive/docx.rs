//! Bounded DOCX/WordprocessingML validation and block parsing.
//!
//! The central format registry uses the bounded package validation here to distinguish DOCX
//! documents from ordinary ZIP archives without parsing the main document body.

use super::{ensure_scan_time, is_safe_entry_name, ArchiveLimits};
use anyhow::{anyhow, bail, Context};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;
use zip::ZipArchive;

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const ROOT_RELATIONSHIPS_PATH: &str = "_rels/.rels";
const MAIN_DOCUMENT_PATH: &str = "word/document.xml";
const DOCUMENT_RELATIONSHIPS_PATH: &str = "word/_rels/document.xml.rels";
const OFFICE_DOCUMENT_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const STRICT_OFFICE_DOCUMENT_RELATIONSHIP: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const IMAGE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const STRICT_IMAGE_RELATIONSHIP: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/image";
const WORD_DOCUMENT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const PACKAGE_METADATA_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DocxBlock {
    Text { text: String },
    Image(DocxImageBlock),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxImageBlock {
    pub image_id: String,
    pub relationship_id: String,
    pub target_path: Option<String>,
    pub mime_type: Option<String>,
    pub alt_text: Option<String>,
    pub width_emu: Option<u64>,
    pub height_emu: Option<u64>,
    pub status: DocxImageStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DocxImageStatus {
    Supported,
    UnsupportedFormat,
    External,
    Missing,
    UnsafePath,
}

#[derive(Debug, Clone)]
struct Relationship {
    target: String,
    kind: String,
    external: bool,
}

#[derive(Debug, Default)]
struct ContentTypes {
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
}

impl ContentTypes {
    fn for_part(&self, path: &str) -> Option<&str> {
        self.overrides
            .get(path)
            .or_else(|| {
                Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .and_then(|extension| self.defaults.get(&extension.to_ascii_lowercase()))
            })
            .map(String::as_str)
    }
}

#[derive(Debug)]
pub struct DocxDocument {
    path: PathBuf,
    limits: ArchiveLimits,
    content_types: ContentTypes,
    relationships: HashMap<String, Relationship>,
    entry_names: HashSet<String>,
}

impl DocxDocument {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::open_with_limits(path, ArchiveLimits::default())
    }

    pub fn open_with_limits(path: &Path, limits: ArchiveLimits) -> anyhow::Result<Self> {
        let started = Instant::now();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| !extension.eq_ignore_ascii_case("docx"))
            .unwrap_or(true)
        {
            bail!("文件不是 .docx 文档");
        }
        if std::fs::metadata(path)?.len() > limits.max_scan_bytes {
            bail!("DOCX 输入超过 {} 字节安全上限", limits.max_scan_bytes);
        }

        let file = File::open(path)?;
        let mut archive = ZipArchive::new(BufReader::new(file)).context("DOCX ZIP 结构无效")?;
        if archive.len() > limits.max_entries {
            bail!("DOCX 包内条目数量超过安全上限");
        }

        let mut entry_names = HashSet::with_capacity(archive.len());
        let mut critical = HashSet::new();
        for index in 0..archive.len() {
            ensure_scan_time(started, limits)?;
            let entry = archive.by_index_raw(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            if !is_safe_entry_name(&name, limits.max_path_bytes) {
                bail!("DOCX 包含不安全条目路径");
            }
            if entry.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170000;
                kind != 0 && kind != 0o100000
            }) {
                bail!("DOCX 包含不支持的特殊条目");
            }
            if entry.encrypted() {
                bail!("DOCX 包含加密部件: {name}");
            }
            if !entry_names.insert(name.clone()) {
                bail!("DOCX 包含重复条目: {name}");
            }
            if matches!(
                name.as_str(),
                CONTENT_TYPES_PATH
                    | ROOT_RELATIONSHIPS_PATH
                    | MAIN_DOCUMENT_PATH
                    | DOCUMENT_RELATIONSHIPS_PATH
            ) {
                critical.insert(name);
            }
        }
        for required in [
            CONTENT_TYPES_PATH,
            ROOT_RELATIONSHIPS_PATH,
            MAIN_DOCUMENT_PATH,
        ] {
            if !critical.contains(required) {
                bail!("DOCX 缺少关键部件: {required}");
            }
        }

        let content_types = parse_content_types(
            read_part(&mut archive, CONTENT_TYPES_PATH, PACKAGE_METADATA_LIMIT)?,
            limits.max_path_bytes,
        )?;
        if content_types.for_part(MAIN_DOCUMENT_PATH) != Some(WORD_DOCUMENT_CONTENT_TYPE) {
            bail!("DOCX 主文档 Content-Type 无效");
        }
        let root_relationships = parse_relationships(read_part(
            &mut archive,
            ROOT_RELATIONSHIPS_PATH,
            PACKAGE_METADATA_LIMIT,
        )?)?;
        let mut main_relationships = root_relationships
            .values()
            .filter(|relationship| is_office_document_relationship(&relationship.kind));
        let main_relationship = main_relationships
            .next()
            .ok_or_else(|| anyhow!("DOCX 缺少主文档关系"))?;
        if main_relationships.next().is_some() {
            bail!("DOCX 包含重复主文档关系");
        }
        if main_relationship.external
            || normalize_part_target("", &main_relationship.target, limits.max_path_bytes)
                .as_deref()
                != Some(MAIN_DOCUMENT_PATH)
        {
            bail!("DOCX 主文档关系目标无效");
        }

        let relationships = if entry_names.contains(DOCUMENT_RELATIONSHIPS_PATH) {
            parse_relationships(read_part(
                &mut archive,
                DOCUMENT_RELATIONSHIPS_PATH,
                PACKAGE_METADATA_LIMIT,
            )?)?
        } else {
            HashMap::new()
        };
        ensure_scan_time(started, limits)?;
        Ok(Self {
            path: path.to_path_buf(),
            limits,
            content_types,
            relationships,
            entry_names,
        })
    }

    #[cfg(test)]
    pub fn parse_blocks(
        &self,
        mut publish: impl FnMut(DocxBlock) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.parse_blocks_until(&mut publish, || false)
    }

    pub fn parse_blocks_until(
        &self,
        mut publish: impl FnMut(DocxBlock) -> anyhow::Result<()>,
        mut cancelled: impl FnMut() -> bool,
    ) -> anyhow::Result<()> {
        let started = Instant::now();
        let file = File::open(&self.path)?;
        let mut archive = ZipArchive::new(BufReader::new(file)).context("DOCX ZIP 结构无效")?;
        let document = archive.by_name(MAIN_DOCUMENT_PATH)?;
        if document.encrypted() {
            bail!("DOCX 主文档已加密");
        }
        let limited = LimitedReader::new(document, self.limits.max_decoded_bytes, "DOCX 主文档");
        let mut reader = Reader::from_reader(BufReader::new(limited));
        reader.trim_text(false);
        reader.expand_empty_elements(true);

        let mut buffer = Vec::with_capacity(8 * 1024);
        let mut state = ParseState::default();
        let mut xml_depth = 0usize;
        let mut root_seen = false;
        let mut body_seen = false;
        let mut inside_body = false;
        let mut output_bytes = 0u64;
        let mut block_count = 0usize;
        loop {
            if cancelled() {
                bail!("DOCX 打开已取消");
            }
            ensure_scan_time(started, self.limits)?;
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(start)) => {
                    let qualified_name = start.name();
                    let name = local_name(qualified_name.as_ref());
                    if xml_depth == 0 {
                        if root_seen || name != b"document" {
                            bail!("DOCX 主文档 XML 根元素无效");
                        }
                        root_seen = true;
                    }
                    xml_depth = xml_depth.saturating_add(1);
                    if xml_depth == 2 && name == b"body" {
                        if body_seen {
                            bail!("DOCX 主文档包含重复正文");
                        }
                        body_seen = true;
                        inside_body = true;
                    } else if inside_body {
                        state.start(&start, self)?;
                    }
                }
                Ok(Event::End(end)) => {
                    let qualified_name = end.name();
                    let name = local_name(qualified_name.as_ref());
                    if inside_body && xml_depth == 2 && name == b"body" {
                        inside_body = false;
                    } else if inside_body {
                        state.end(name)?;
                    }
                    xml_depth = xml_depth.saturating_sub(1);
                }
                Ok(Event::Text(text)) if inside_body && state.capture_text() => {
                    let text = text.unescape().context("DOCX 文本实体无效")?;
                    state.push_text(&text);
                }
                Ok(Event::CData(text)) if inside_body && state.capture_text() => {
                    state.push_text(&String::from_utf8_lossy(text.as_ref()));
                }
                Ok(Event::DocType(_)) => bail!("DOCX XML 禁止 DOCTYPE 或外部实体"),
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(error) => return Err(anyhow!("DOCX 主文档 XML 无效: {error}")),
            }
            buffer.clear();
            while let Some(block) = state.ready.pop_front() {
                let block_bytes = match &block {
                    DocxBlock::Text { text } => text.len() as u64,
                    DocxBlock::Image(image) => image
                        .alt_text
                        .as_ref()
                        .map(|text| text.len() as u64)
                        .unwrap_or(0),
                };
                output_bytes = output_bytes.saturating_add(block_bytes);
                block_count = block_count.saturating_add(1);
                if output_bytes > self.limits.max_decoded_bytes {
                    bail!("DOCX 转换文本超过安全上限");
                }
                if block_count > self.limits.max_entries {
                    bail!("DOCX 图文块数量超过安全上限");
                }
                publish(block)?;
            }
        }
        if !root_seen
            || !body_seen
            || xml_depth != 0
            || inside_body
            || state.paragraph_depth != 0
            || state.table_depth != 0
        {
            bail!(
                "DOCX 主文档 XML 结构不完整 (root={root_seen}, body={body_seen}, xml_depth={xml_depth}, inside_body={inside_body}, paragraph_depth={}, table_depth={})",
                state.paragraph_depth,
                state.table_depth
            );
        }
        ensure_scan_time(started, self.limits)?;
        Ok(())
    }

    pub fn read_supported_image(
        &self,
        target_path: &str,
        expected_mime: &str,
    ) -> anyhow::Result<Vec<u8>> {
        if !self.entry_names.contains(target_path)
            || !is_safe_entry_name(target_path, self.limits.max_path_bytes)
        {
            bail!("DOCX 图片路径无效");
        }
        if !matches!(expected_mime, "image/png" | "image/jpeg")
            || self.content_types.for_part(target_path) != Some(expected_mime)
        {
            bail!("DOCX 图片 MIME 不受支持或与部件声明不一致");
        }
        let file = File::open(&self.path)?;
        let mut archive = ZipArchive::new(BufReader::new(file)).context("DOCX ZIP 结构无效")?;
        let entry = archive.by_name(target_path)?;
        if entry.encrypted() {
            bail!("DOCX 图片已加密");
        }
        let mut limited = LimitedReader::new(entry, 16 * 1024 * 1024, "DOCX 图片");
        let mut bytes = Vec::new();
        limited.read_to_end(&mut bytes)?;
        let (actual_mime, width, height) = image_metadata(&bytes)?;
        if actual_mime != expected_mime {
            bail!("DOCX 图片 magic 与 MIME 不一致");
        }
        if u64::from(width).saturating_mul(u64::from(height)) > 32_000_000 {
            bail!("DOCX 图片像素超过 32 MP 安全上限");
        }
        Ok(bytes)
    }
}

fn image_metadata(bytes: &[u8]) -> anyhow::Result<(&'static str, u32, u32)> {
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if &bytes[12..16] != b"IHDR" {
            bail!("PNG 缺少 IHDR");
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if width == 0 || height == 0 {
            bail!("PNG 尺寸无效");
        }
        return Ok(("image/png", width, height));
    }
    if bytes.len() >= 4 && bytes.starts_with(&[0xff, 0xd8]) {
        let mut offset = 2usize;
        while offset + 4 <= bytes.len() {
            if bytes[offset] != 0xff {
                offset += 1;
                continue;
            }
            while offset < bytes.len() && bytes[offset] == 0xff {
                offset += 1;
            }
            if offset >= bytes.len() {
                break;
            }
            let marker = bytes[offset];
            offset += 1;
            if matches!(marker, 0xd8 | 0xd9 | 0x01) || (0xd0..=0xd7).contains(&marker) {
                continue;
            }
            if offset + 2 > bytes.len() {
                break;
            }
            let length = usize::from(u16::from_be_bytes([bytes[offset], bytes[offset + 1]]));
            if length < 2 || offset + length > bytes.len() {
                break;
            }
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                if length < 7 {
                    break;
                }
                let height = u32::from(u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]));
                let width = u32::from(u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]));
                if width == 0 || height == 0 {
                    bail!("JPEG 尺寸无效");
                }
                return Ok(("image/jpeg", width, height));
            }
            offset += length;
        }
        bail!("JPEG 缺少有效尺寸标记");
    }
    bail!("DOCX 图片 magic 不受支持")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use zip::write::SimpleFileOptions;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Default Extension="jpg" ContentType="image/jpeg"/>
  <Default Extension="gif" ContentType="image/gif"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
    const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    const EMPTY_DOCUMENT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn create(document: &str, relationships: Option<&str>, extra: &[(&str, &[u8])]) -> Self {
            Self::create_with_parts(
                &[
                    (CONTENT_TYPES_PATH, CONTENT_TYPES.as_bytes()),
                    (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
                    (MAIN_DOCUMENT_PATH, document.as_bytes()),
                ],
                relationships,
                extra,
                "docx",
            )
        }

        fn create_with_parts(
            required: &[(&str, &[u8])],
            relationships: Option<&str>,
            extra: &[(&str, &[u8])],
            extension: &str,
        ) -> Self {
            let path = std::env::temp_dir().join(format!(
                "logcrate-docx-test-{}-{}.{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed),
                extension
            ));
            let file = File::create(&path).expect("create DOCX fixture");
            let mut archive = zip::ZipWriter::new(file);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, bytes) in required {
                archive.start_file(*name, options).expect("start DOCX part");
                archive.write_all(bytes).expect("write DOCX part");
            }
            if let Some(relationships) = relationships {
                archive
                    .start_file(DOCUMENT_RELATIONSHIPS_PATH, options)
                    .expect("start document relationships");
                archive
                    .write_all(relationships.as_bytes())
                    .expect("write document relationships");
            }
            for (name, bytes) in extra {
                archive
                    .start_file(*name, options)
                    .expect("start extra part");
                archive.write_all(bytes).expect("write extra part");
            }
            archive.finish().expect("finish DOCX fixture");
            Self { path }
        }

        fn blocks(&self) -> anyhow::Result<Vec<DocxBlock>> {
            let document = DocxDocument::open(&self.path)?;
            let mut blocks = Vec::new();
            document.parse_blocks(|block| {
                blocks.push(block);
                Ok(())
            })?;
            Ok(blocks)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn document(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body>{body}</w:body></w:document>"#
        )
    }

    fn assert_open_error(fixture: &Fixture, expected: &str) {
        let error = DocxDocument::open(&fixture.path).expect_err("fixture must be rejected");
        assert!(
            error.to_string().contains(expected),
            "unexpected error: {error:#}"
        );
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 24];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        vec![
            0xff,
            0xd8,
            0xff,
            0xc0,
            0x00,
            0x07,
            0x08,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            0xff,
            0xd9,
        ]
    }

    #[test]
    fn validates_png_and_jpeg_magic_dimensions_and_pixel_limit() {
        assert_eq!(image_metadata(&png(2, 3)).unwrap(), ("image/png", 2, 3));
        assert_eq!(
            image_metadata(&jpeg(320, 240)).unwrap(),
            ("image/jpeg", 320, 240)
        );
        assert!(image_metadata(b"GIF89a").is_err());
        assert!(image_metadata(&png(0, 3)).is_err());

        let xml = document(
            r#"<w:p><w:r><w:drawing><wp:inline><a:blip r:embed="img"/></wp:inline></w:drawing></w:r></w:p>"#,
        );
        let relationships = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/large.png"/></Relationships>"#;
        let large = png(8000, 5000);
        let fixture = Fixture::create(
            &xml,
            Some(relationships),
            &[("word/media/large.png", &large)],
        );
        let document = DocxDocument::open(&fixture.path).unwrap();
        let error = document
            .read_supported_image("word/media/large.png", "image/png")
            .expect_err("pixel limit must fail");
        assert!(error.to_string().contains("32 MP"));
        assert!(document
            .read_supported_image("word/media/large.png", "image/jpeg")
            .is_err());

        let mut oversized = png(2, 3);
        oversized.resize(16 * 1024 * 1024 + 1, 0);
        let oversized_fixture = Fixture::create(
            &xml,
            Some(relationships),
            &[("word/media/large.png", &oversized)],
        );
        let oversized_document = DocxDocument::open(&oversized_fixture.path).unwrap();
        let error = oversized_document
            .read_supported_image("word/media/large.png", "image/png")
            .expect_err("decoded byte limit must fail");
        assert!(error.to_string().contains("16 MiB") || error.to_string().contains("安全上限"));
    }

    #[test]
    fn parses_runs_empty_paragraphs_breaks_tabs_and_tables() {
        let xml = document(
            r#"<w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>world</w:t><w:tab/><w:t>tab</w:t><w:br/><w:t>line</w:t></w:r></w:p>
<w:p/>
<w:tbl>
 <w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
 <w:tr><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr>
</w:tbl>
<w:p><w:r><w:del><w:t>deleted</w:t></w:del><w:instrText>field</w:instrText><w:t>tail</w:t></w:r></w:p>"#,
        );
        let fixture = Fixture::create(&xml, None, &[]);
        assert_eq!(
            fixture.blocks().expect("parse blocks"),
            vec![
                DocxBlock::Text {
                    text: "Hello world\ttab\nline\n".into()
                },
                DocxBlock::Text { text: "\n".into() },
                DocxBlock::Text {
                    text: "A1 A2\tB\n".into()
                },
                DocxBlock::Text {
                    text: "C\t\n".into()
                },
                DocxBlock::Text {
                    text: "tail\n".into()
                },
            ]
        );
    }

    #[test]
    fn preserves_paragraph_image_anchor_order_and_metadata() {
        let relationships = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="img1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/shot.png"/>
<Relationship Id="img2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/photo.jpg"/>
</Relationships>"#;
        let xml = document(
            r#"<w:p><w:r><w:t>before</w:t><w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Shot" descr="screen shot"/><a:blip r:embed="img1"/></wp:inline></w:drawing><w:t>after</w:t></w:r></w:p>
<w:p><w:r><w:drawing><wp:anchor><wp:docPr id="2" name="Photo"/><a:blip r:embed="img2"/></wp:anchor></w:drawing></w:r></w:p>"#,
        );
        let fixture = Fixture::create(
            &xml,
            Some(relationships),
            &[
                ("word/media/shot.png", b"png"),
                ("word/media/photo.jpg", b"jpg"),
            ],
        );
        let blocks = fixture.blocks().expect("parse image blocks");
        assert_eq!(blocks.len(), 5);
        assert_eq!(
            blocks[0],
            DocxBlock::Text {
                text: "before".into()
            }
        );
        assert_eq!(
            blocks[2],
            DocxBlock::Text {
                text: "after\n".into()
            }
        );
        assert_eq!(blocks[4], DocxBlock::Text { text: "\n".into() });
        let DocxBlock::Image(first) = &blocks[1] else {
            panic!("expected first image")
        };
        assert_eq!(first.status, DocxImageStatus::Supported);
        assert_eq!(first.target_path.as_deref(), Some("word/media/shot.png"));
        assert_eq!(first.mime_type.as_deref(), Some("image/png"));
        assert_eq!(first.alt_text.as_deref(), Some("screen shot"));
        assert_eq!(
            (first.width_emu, first.height_emu),
            (Some(914400), Some(457200))
        );
        let DocxBlock::Image(second) = &blocks[3] else {
            panic!("expected second image")
        };
        assert_eq!(second.image_id, "image-2");
        assert_eq!(second.alt_text.as_deref(), Some("Photo"));
        assert_eq!(second.mime_type.as_deref(), Some("image/jpeg"));
    }

    #[test]
    fn classifies_external_unsupported_missing_and_unsafe_images() {
        let relationships = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="external" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.invalid/a.png" TargetMode="External"/>
<Relationship Id="gif" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/a.gif"/>
<Relationship Id="missing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/missing.png"/>
<Relationship Id="unsafe" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../../outside.png"/>
</Relationships>"#;
        let xml = document(
            r#"<w:p><w:r>
<w:drawing><wp:inline><a:blip r:link="external"/></wp:inline></w:drawing>
<w:drawing><wp:inline><a:blip r:embed="gif"/></wp:inline></w:drawing>
<w:drawing><wp:inline><a:blip r:embed="missing"/></wp:inline></w:drawing>
<w:drawing><wp:inline><a:blip r:embed="unsafe"/></wp:inline></w:drawing>
<w:drawing><wp:inline><a:blip r:embed="unknown"/></wp:inline></w:drawing>
</w:r></w:p>"#,
        );
        let fixture = Fixture::create(&xml, Some(relationships), &[("word/media/a.gif", b"gif")]);
        let statuses: Vec<DocxImageStatus> = fixture
            .blocks()
            .expect("parse placeholders")
            .into_iter()
            .filter_map(|block| match block {
                DocxBlock::Image(image) => Some(image.status),
                DocxBlock::Text { .. } => None,
            })
            .collect();
        assert_eq!(
            statuses,
            vec![
                DocxImageStatus::External,
                DocxImageStatus::UnsupportedFormat,
                DocxImageStatus::Missing,
                DocxImageStatus::UnsafePath,
                DocxImageStatus::Missing,
            ]
        );
    }

    #[test]
    fn rejects_wrong_suffix_missing_parts_and_invalid_content_type() {
        let xml = document("<w:p/>");
        let wrong_suffix = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, CONTENT_TYPES.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "zip",
        );
        assert_open_error(&wrong_suffix, "不是 .docx");

        let missing_document = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, CONTENT_TYPES.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        assert_open_error(&missing_document, "缺少关键部件");

        let bad_content_types = CONTENT_TYPES.replace(WORD_DOCUMENT_CONTENT_TYPE, "text/xml");
        let invalid_type = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, bad_content_types.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        assert_open_error(&invalid_type, "Content-Type 无效");

        let no_main_relationship = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, CONTENT_TYPES.as_bytes()),
                (
                    ROOT_RELATIONSHIPS_PATH,
                    EMPTY_DOCUMENT_RELATIONSHIPS.as_bytes(),
                ),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        assert_open_error(&no_main_relationship, "缺少主文档关系");
    }

    #[test]
    fn rejects_duplicate_critical_metadata_doctype_and_malformed_xml() {
        let xml = document("<w:p/>");
        let duplicate_content_types = CONTENT_TYPES.replace(
            "</Types>",
            &format!(
                r#"<Override PartName="/word/document.xml" ContentType="{WORD_DOCUMENT_CONTENT_TYPE}"/></Types>"#
            ),
        );
        let duplicate = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, duplicate_content_types.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        assert_open_error(&duplicate, "覆盖项无效或重复");

        let doctype = format!("<!DOCTYPE document [<!ENTITY x SYSTEM \"file:///x\">]>{xml}");
        let fixture = Fixture::create(&doctype, None, &[]);
        let document = DocxDocument::open(&fixture.path).expect("metadata is valid");
        let error = document
            .parse_blocks(|_| Ok(()))
            .expect_err("DOCTYPE must be rejected");
        assert!(error.to_string().contains("DOCTYPE"));

        let malformed = Fixture::create("<w:document><w:body><w:p>", None, &[]);
        let error = malformed.blocks().expect_err("malformed XML must fail");
        assert!(error.to_string().contains("XML"));
    }

    #[test]
    fn enforces_input_metadata_output_block_and_time_limits() {
        let xml = document("<w:p><w:r><w:t>abcdef</w:t></w:r></w:p>");
        let fixture = Fixture::create(&xml, Some(EMPTY_DOCUMENT_RELATIONSHIPS), &[]);

        let limits = ArchiveLimits {
            max_scan_bytes: 1,
            ..ArchiveLimits::default()
        };
        let error = DocxDocument::open_with_limits(&fixture.path, limits)
            .expect_err("input limit must fail");
        assert!(error.to_string().contains("输入超过"));

        let limits = ArchiveLimits {
            max_decoded_bytes: 5,
            ..ArchiveLimits::default()
        };
        let document = DocxDocument::open_with_limits(&fixture.path, limits).expect("open fixture");
        let error = document
            .parse_blocks(|_| Ok(()))
            .expect_err("decoded XML/output limit must fail");
        assert!(error.to_string().contains("安全上限"));

        let limits = ArchiveLimits {
            max_entries: 1,
            ..ArchiveLimits::default()
        };
        let error = DocxDocument::open_with_limits(&fixture.path, limits)
            .expect_err("entry limit must fail");
        assert!(error.to_string().contains("条目数量"));

        let limits = ArchiveLimits {
            max_scan_duration: Duration::ZERO,
            ..ArchiveLimits::default()
        };
        let error = DocxDocument::open_with_limits(&fixture.path, limits)
            .expect_err("time limit must fail");
        assert!(error.to_string().contains("时间上限"));
    }

    #[test]
    fn enforces_converted_output_block_limit_and_callback_failure() {
        let xml = document(
            "<w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p><w:p><w:r><w:t>c</w:t></w:r></w:p><w:p><w:r><w:t>d</w:t></w:r></w:p>",
        );
        let fixture = Fixture::create(&xml, None, &[]);

        let limits = ArchiveLimits {
            max_entries: 3,
            ..ArchiveLimits::default()
        };
        let document = DocxDocument::open_with_limits(&fixture.path, limits).expect("open fixture");
        let mut published = 0usize;
        let error = document
            .parse_blocks(|_| {
                published += 1;
                Ok(())
            })
            .expect_err("block limit must fail");
        assert_eq!(published, 3);
        assert!(error.to_string().contains("图文块数量"));

        let document = DocxDocument::open(&fixture.path).expect("open fixture");
        let error = document
            .parse_blocks(|_| bail!("consumer stopped"))
            .expect_err("callback failure must propagate");
        assert!(error.to_string().contains("consumer stopped"));
    }

    #[test]
    fn accepts_strict_relationship_namespaces_and_rejects_duplicate_main_relationships() {
        let strict_root = format!(
            r#"<Relationships xmlns="http://purl.oclc.org/ooxml/package/relationships"><Relationship Id="rId1" Type="{STRICT_OFFICE_DOCUMENT_RELATIONSHIP}" Target="word/document.xml"/></Relationships>"#
        );
        let strict_document_rels = format!(
            r#"<Relationships xmlns="http://purl.oclc.org/ooxml/package/relationships"><Relationship Id="img" Type="{STRICT_IMAGE_RELATIONSHIP}" Target="media/strict.png"/></Relationships>"#
        );
        let xml = document(
            r#"<w:p><w:r><w:drawing><wp:inline><a:blip r:embed="img"/></wp:inline></w:drawing></w:r></w:p>"#,
        );
        let fixture = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, CONTENT_TYPES.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, strict_root.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            Some(&strict_document_rels),
            &[("word/media/strict.png", b"png")],
            "docx",
        );
        let blocks = fixture.blocks().expect("strict DOCX relationships");
        assert!(matches!(
            &blocks[0],
            DocxBlock::Image(DocxImageBlock {
                status: DocxImageStatus::Supported,
                ..
            })
        ));

        let duplicate_root = format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="{OFFICE_DOCUMENT_RELATIONSHIP}" Target="word/document.xml"/>
<Relationship Id="rId2" Type="{OFFICE_DOCUMENT_RELATIONSHIP}" Target="word/document.xml"/>
</Relationships>"#
        );
        let duplicate = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, CONTENT_TYPES.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, duplicate_root.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        assert_open_error(&duplicate, "重复主文档关系");
    }

    #[test]
    fn rejects_unsafe_archive_entries_and_metadata_doctype() {
        let xml = document("<w:p/>");
        let unsafe_entry = Fixture::create(&xml, None, &[("../escape.png", b"bad")]);
        assert_open_error(&unsafe_entry, "不安全条目路径");

        let content_types = CONTENT_TYPES.replacen(
            "<Types",
            "<!DOCTYPE Types [<!ENTITY x SYSTEM \"file:///x\">]><Types",
            1,
        );
        let fixture = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, content_types.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        assert_open_error(&fixture, "DOCTYPE");
    }

    #[test]
    fn applies_configured_path_limit_to_parts_and_relationship_targets() {
        let xml = document(
            r#"<w:p><w:r><w:drawing><wp:inline><a:blip r:embed="img"/></wp:inline></w:drawing></w:r></w:p>"#,
        );
        let relationships = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/this-target-is-longer-than-the-document-relationships-part-name.png"/></Relationships>"#;
        let fixture = Fixture::create(&xml, Some(relationships), &[]);
        let limits = ArchiveLimits {
            max_path_bytes: DOCUMENT_RELATIONSHIPS_PATH.len(),
            ..ArchiveLimits::default()
        };
        let document =
            DocxDocument::open_with_limits(&fixture.path, limits).expect("critical paths fit");
        let blocks = {
            let mut blocks = Vec::new();
            document
                .parse_blocks(|block| {
                    blocks.push(block);
                    Ok(())
                })
                .expect("parse blocks");
            blocks
        };
        let image = blocks
            .into_iter()
            .find_map(|block| match block {
                DocxBlock::Image(image) => Some(image),
                DocxBlock::Text { .. } => None,
            })
            .expect("image block");
        assert_eq!(image.status, DocxImageStatus::UnsafePath);
        assert_eq!(image.target_path, None);
    }

    #[test]
    fn rejects_corrupt_zip_and_oversized_package_metadata() {
        let corrupt_path = std::env::temp_dir().join(format!(
            "logcrate-docx-test-{}-{}.docx",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&corrupt_path, b"not a zip").expect("write corrupt fixture");
        let error = DocxDocument::open(&corrupt_path).expect_err("corrupt ZIP must fail");
        assert!(error.to_string().contains("ZIP 结构无效"));
        let _ = std::fs::remove_file(corrupt_path);

        let oversized_content_types = format!(
            "{}<!--{}-->",
            CONTENT_TYPES,
            "x".repeat(PACKAGE_METADATA_LIMIT as usize)
        );
        let xml = document("<w:p/>");
        let fixture = Fixture::create_with_parts(
            &[
                (CONTENT_TYPES_PATH, oversized_content_types.as_bytes()),
                (ROOT_RELATIONSHIPS_PATH, ROOT_RELATIONSHIPS.as_bytes()),
                (MAIN_DOCUMENT_PATH, xml.as_bytes()),
            ],
            None,
            &[],
            "docx",
        );
        let error = DocxDocument::open(&fixture.path).expect_err("metadata limit must fail");
        assert!(format!("{error:#}").contains("包元数据超过安全上限"));
    }
}

fn read_part<'a>(
    archive: &'a mut ZipArchive<BufReader<File>>,
    path: &str,
    limit: u64,
) -> anyhow::Result<LimitedReader<zip::read::ZipFile<'a>>> {
    let entry = archive.by_name(path)?;
    if entry.encrypted() {
        bail!("DOCX 部件已加密: {path}");
    }
    Ok(LimitedReader::new(entry, limit, "DOCX 包元数据"))
}

struct LimitedReader<R> {
    inner: R,
    remaining: u64,
    label: &'static str,
}

impl<R> LimitedReader<R> {
    fn new(inner: R, limit: u64, label: &'static str) -> Self {
        Self {
            inner,
            remaining: limit,
            label,
        }
    }
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let allowed = self.remaining.min(buffer.len() as u64) as usize;
        if allowed == 0 {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}超过安全上限", self.label),
                )),
            };
        }
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn parse_content_types(reader: impl Read, max_path_bytes: usize) -> anyhow::Result<ContentTypes> {
    let mut xml = Reader::from_reader(BufReader::new(reader));
    xml.trim_text(true);
    xml.expand_empty_elements(true);
    let mut buffer = Vec::new();
    let mut result = ContentTypes::default();
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        match xml.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) => {
                let qualified_name = start.name();
                let name = local_name(qualified_name.as_ref());
                if depth == 0 {
                    if root_seen || name != b"Types" {
                        bail!("DOCX Content-Type XML 根元素无效");
                    }
                    root_seen = true;
                } else if depth == 1 && name == b"Default" {
                    let extension = required_attribute(&start, b"Extension")?.to_ascii_lowercase();
                    let content_type = required_attribute(&start, b"ContentType")?;
                    if extension.is_empty()
                        || result.defaults.insert(extension, content_type).is_some()
                    {
                        bail!("DOCX Content-Type 包含无效或重复默认项");
                    }
                } else if depth == 1 && name == b"Override" {
                    let part = required_attribute(&start, b"PartName")?;
                    let part = part.strip_prefix('/').unwrap_or(&part).to_string();
                    if !is_safe_entry_name(&part, max_path_bytes)
                        || result
                            .overrides
                            .insert(part, required_attribute(&start, b"ContentType")?)
                            .is_some()
                    {
                        bail!("DOCX Content-Type 覆盖项无效或重复");
                    }
                }
                depth = depth.saturating_add(1);
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::DocType(_)) => bail!("DOCX XML 禁止 DOCTYPE 或外部实体"),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(anyhow!("DOCX Content-Type XML 无效: {error}")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        bail!("DOCX Content-Type XML 结构不完整");
    }
    Ok(result)
}

fn parse_relationships(reader: impl Read) -> anyhow::Result<HashMap<String, Relationship>> {
    let mut xml = Reader::from_reader(BufReader::new(reader));
    xml.trim_text(true);
    xml.expand_empty_elements(true);
    let mut buffer = Vec::new();
    let mut result = HashMap::new();
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        match xml.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) => {
                let qualified_name = start.name();
                let name = local_name(qualified_name.as_ref());
                if depth == 0 {
                    if root_seen || name != b"Relationships" {
                        bail!("DOCX 关系 XML 根元素无效");
                    }
                    root_seen = true;
                } else if depth == 1 && name == b"Relationship" {
                    let id = required_attribute(&start, b"Id")?;
                    let relationship = Relationship {
                        target: required_attribute(&start, b"Target")?,
                        kind: required_attribute(&start, b"Type")?,
                        external: attribute(&start, b"TargetMode")?
                            .map(|value| value.eq_ignore_ascii_case("external"))
                            .unwrap_or(false),
                    };
                    if id.is_empty() || result.insert(id, relationship).is_some() {
                        bail!("DOCX 关系 ID 为空或重复");
                    }
                }
                depth = depth.saturating_add(1);
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::DocType(_)) => bail!("DOCX XML 禁止 DOCTYPE 或外部实体"),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(anyhow!("DOCX 关系 XML 无效: {error}")),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        bail!("DOCX 关系 XML 结构不完整");
    }
    Ok(result)
}

fn required_attribute(start: &BytesStart<'_>, name: &[u8]) -> anyhow::Result<String> {
    attribute(start, name)?.ok_or_else(|| anyhow!("DOCX XML 缺少必需属性"))
}

fn attribute(start: &BytesStart<'_>, name: &[u8]) -> anyhow::Result<Option<String>> {
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.context("DOCX XML 属性无效")?;
        if local_name(attribute.key.as_ref()) == name {
            return Ok(Some(
                attribute
                    .unescape_value()
                    .context("DOCX XML 属性实体无效")?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn is_office_document_relationship(kind: &str) -> bool {
    matches!(
        kind,
        OFFICE_DOCUMENT_RELATIONSHIP | STRICT_OFFICE_DOCUMENT_RELATIONSHIP
    )
}

fn is_image_relationship(kind: &str) -> bool {
    matches!(kind, IMAGE_RELATIONSHIP | STRICT_IMAGE_RELATIONSHIP)
}

fn normalize_part_target(base: &str, target: &str, max_path_bytes: usize) -> Option<String> {
    if target.is_empty() || target.contains(['\\', '\0']) || target.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').filter(|part| !part.is_empty()).collect()
    };
    for component in Path::new(target).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    let normalized = parts.join("/");
    is_safe_entry_name(&normalized, max_path_bytes).then_some(normalized)
}

#[derive(Debug, Default)]
struct DrawingState {
    alt_text: Option<String>,
    width_emu: Option<u64>,
    height_emu: Option<u64>,
}

#[derive(Debug, Default)]
struct ParseState {
    ready: std::collections::VecDeque<DocxBlock>,
    paragraph_depth: usize,
    table_depth: usize,
    row_depth: usize,
    cell_depth: usize,
    text_depth: usize,
    deleted_depth: usize,
    instruction_depth: usize,
    paragraph_text: String,
    row_cells: Vec<String>,
    cell_paragraphs: Vec<String>,
    row_images: Vec<DocxImageBlock>,
    drawing: Option<DrawingState>,
    image_sequence: usize,
}

impl ParseState {
    fn start(&mut self, start: &BytesStart<'_>, document: &DocxDocument) -> anyhow::Result<()> {
        match local_name(start.name().as_ref()) {
            b"tbl" => self.table_depth += 1,
            b"tr" if self.table_depth > 0 => {
                self.row_depth += 1;
                if self.row_depth == 1 {
                    self.row_cells.clear();
                    self.row_images.clear();
                }
            }
            b"tc" if self.row_depth > 0 => {
                self.cell_depth += 1;
                if self.cell_depth == 1 {
                    self.cell_paragraphs.clear();
                }
            }
            b"p" => {
                self.paragraph_depth += 1;
                if self.paragraph_depth == 1 {
                    self.paragraph_text.clear();
                }
            }
            b"t" => self.text_depth += 1,
            b"del" => self.deleted_depth += 1,
            b"instrText" => self.instruction_depth += 1,
            b"tab" if self.paragraph_depth > 0 => self.paragraph_text.push('\t'),
            b"br" | b"cr" if self.paragraph_depth > 0 => self.paragraph_text.push('\n'),
            b"inline" | b"anchor" => self.drawing = Some(DrawingState::default()),
            b"extent" => {
                if let Some(drawing) = &mut self.drawing {
                    drawing.width_emu =
                        attribute(start, b"cx")?.and_then(|value| value.parse().ok());
                    drawing.height_emu =
                        attribute(start, b"cy")?.and_then(|value| value.parse().ok());
                }
            }
            b"docPr" => {
                if let Some(drawing) = &mut self.drawing {
                    drawing.alt_text = attribute(start, b"descr")?
                        .or(attribute(start, b"title")?)
                        .or(attribute(start, b"name")?)
                        .filter(|text| !text.trim().is_empty());
                }
            }
            b"blip" if self.drawing.is_some() => {
                let embedded = attribute(start, b"embed")?;
                let linked = attribute(start, b"link")?;
                let explicitly_linked = linked.is_some();
                if let Some(relationship_id) = embedded.or(linked) {
                    let image = self.image_block(document, relationship_id, explicitly_linked);
                    if self.row_depth > 0 {
                        self.row_images.push(image);
                    } else {
                        self.flush_paragraph_fragment(false);
                        self.ready.push_back(DocxBlock::Image(image));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn end(&mut self, name: &[u8]) -> anyhow::Result<()> {
        match name {
            b"t" => self.text_depth = self.text_depth.saturating_sub(1),
            b"del" => self.deleted_depth = self.deleted_depth.saturating_sub(1),
            b"instrText" => self.instruction_depth = self.instruction_depth.saturating_sub(1),
            b"inline" | b"anchor" => self.drawing = None,
            b"p" => {
                self.paragraph_depth = self.paragraph_depth.saturating_sub(1);
                if self.paragraph_depth == 0 {
                    if self.cell_depth > 0 {
                        self.cell_paragraphs
                            .push(std::mem::take(&mut self.paragraph_text));
                    } else if self.row_depth == 0 {
                        self.flush_paragraph_fragment(true);
                    }
                }
            }
            b"tc" if self.cell_depth > 0 => {
                self.cell_depth -= 1;
                if self.cell_depth == 0 {
                    self.row_cells.push(self.cell_paragraphs.join(" "));
                    self.cell_paragraphs.clear();
                }
            }
            b"tr" if self.row_depth > 0 => {
                self.row_depth -= 1;
                if self.row_depth == 0 {
                    self.ready.push_back(DocxBlock::Text {
                        text: format!("{}\n", self.row_cells.join("\t")),
                    });
                    for image in self.row_images.drain(..) {
                        self.ready.push_back(DocxBlock::Image(image));
                    }
                }
            }
            b"tbl" if self.table_depth > 0 => self.table_depth -= 1,
            _ => {}
        }
        Ok(())
    }

    fn capture_text(&self) -> bool {
        self.paragraph_depth > 0
            && self.text_depth > 0
            && self.deleted_depth == 0
            && self.instruction_depth == 0
    }

    fn push_text(&mut self, text: &str) {
        self.paragraph_text.push_str(text);
    }

    fn flush_paragraph_fragment(&mut self, paragraph_end: bool) {
        if paragraph_end {
            self.paragraph_text.push('\n');
        }
        if !self.paragraph_text.is_empty() {
            self.ready.push_back(DocxBlock::Text {
                text: std::mem::take(&mut self.paragraph_text),
            });
        }
    }

    fn image_block(
        &mut self,
        document: &DocxDocument,
        relationship_id: String,
        explicitly_linked: bool,
    ) -> DocxImageBlock {
        self.image_sequence += 1;
        let drawing = self.drawing.as_ref().expect("drawing state exists");
        let relationship = document.relationships.get(&relationship_id);
        let (target_path, mime_type, status) = match relationship {
            None => (None, None, DocxImageStatus::Missing),
            Some(relationship) if explicitly_linked || relationship.external => {
                (None, None, DocxImageStatus::External)
            }
            Some(relationship) if !is_image_relationship(&relationship.kind) => {
                (None, None, DocxImageStatus::Missing)
            }
            Some(relationship) => match normalize_part_target(
                "word",
                &relationship.target,
                document.limits.max_path_bytes,
            ) {
                None => (None, None, DocxImageStatus::UnsafePath),
                Some(path) if !document.entry_names.contains(&path) => {
                    (Some(path), None, DocxImageStatus::Missing)
                }
                Some(path) => {
                    let mime = document.content_types.for_part(&path).map(str::to_string);
                    let status = if matches!(mime.as_deref(), Some("image/png" | "image/jpeg")) {
                        DocxImageStatus::Supported
                    } else {
                        DocxImageStatus::UnsupportedFormat
                    };
                    (Some(path), mime, status)
                }
            },
        };
        DocxImageBlock {
            image_id: format!("image-{}", self.image_sequence),
            relationship_id,
            target_path,
            mime_type,
            alt_text: drawing.alt_text.clone(),
            width_emu: drawing.width_emu,
            height_emu: drawing.height_emu,
            status,
        }
    }
}
