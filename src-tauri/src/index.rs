//! Incremental line indexing, windowed reads, decoding, and session lifecycle.

use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::log_fields::{
    analyze_layout, extend_field_lines, scan_field_lines, LayoutAnalysis, LogFieldCondition,
    LogFieldLayout, LogFieldScanResult, LogFieldStatistics, SamplingPhase,
};

pub const MAX_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_SESSIONS: usize = 5;

static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub line_no: u64,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub session_id: String,
    pub entry_path: String,
    pub size: u64,
    pub indexing: bool,
    pub encoding: String,
    pub evicted_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub session_id: String,
    pub percent: u8,
    pub indexed_lines: u64,
    pub done: bool,
    pub failed: bool,
    pub detected_encoding: String,
    pub effective_encoding: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodingProgress {
    pub session_id: String,
    pub generation: u64,
    pub percent: u8,
    pub encoding: String,
    pub line_count: u64,
    pub done: bool,
    pub failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotExportResult {
    pub bytes: u64,
    pub complete: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchRequest {
    pub query: String,
    /// Zero-based line used as the first search position.
    pub start_line: u64,
    /// UTF-16 code-unit offset. None means line start forward and line end in reverse.
    pub start_column: Option<u64>,
    pub reverse: bool,
    pub whole_word: bool,
    pub case_sensitive: bool,
    pub wrap: bool,
    #[serde(default)]
    pub field_view: Option<LogFieldSearchView>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogFieldResultMode {
    Compact,
    Highlight,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldSearchView {
    pub generation: u64,
    pub mode: LogFieldResultMode,
    pub include_unparsed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldFilterRequest {
    pub layout: LogFieldLayout,
    pub conditions: Vec<LogFieldCondition>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldProgress {
    pub session_id: String,
    pub generation: u64,
    pub scanned_lines: u64,
    pub matched_lines: u64,
    pub unparsed_lines: u64,
    pub total_lines: u64,
    pub done: bool,
    pub failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldStatus {
    pub generation: u64,
    pub layout: LogFieldLayout,
    pub conditions: Vec<LogFieldCondition>,
    pub statistics: Vec<LogFieldStatistics>,
    pub scanned_lines: u64,
    pub matched_lines: u64,
    pub unparsed_lines: u64,
    pub total_lines: u64,
    pub done: bool,
    pub failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldMarkedLine {
    #[serde(flatten)]
    pub line: LogLine,
    pub field_matched: bool,
    pub field_unparsed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFieldAnchorResult {
    pub view_index: u64,
    pub line_no: u64,
}

#[derive(Debug, Clone)]
struct LogFieldSessionState {
    status: LogFieldStatus,
    matched_original_lines: Vec<u64>,
    unparsed_original_lines: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchMatch {
    pub line_no: u64,
    pub start_column: u64,
    pub end_column: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchResult {
    #[serde(rename = "match")]
    pub matched: Option<LogSearchMatch>,
    pub wrapped: bool,
    pub reached_boundary: bool,
    pub indexed_lines: u64,
    pub indexing: bool,
}

pub struct Session {
    cache_path: PathBuf,
    cache_io: Arc<Mutex<()>>,
    /// Starts of complete lines, followed by the current readable boundary.
    offsets: Vec<u64>,
    detected_encoding: Option<&'static Encoding>,
    effective_encoding: &'static Encoding,
    indexing: bool,
    encoding_generation: u64,
    field_generation: u64,
    field_state: Option<LogFieldSessionState>,
    cancel: Arc<AtomicBool>,
}

pub struct LogFieldBuild {
    session_id: String,
    session: Arc<Mutex<Session>>,
    generation: u64,
    request: LogFieldFilterRequest,
    total_lines: u64,
    initial: Option<LogFieldScanResult>,
}

impl LogFieldBuild {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl Session {
    pub fn line_count(&self) -> u64 {
        self.offsets.len().saturating_sub(1) as u64
    }
}

pub struct EncodingChange {
    session_id: String,
    session: Arc<Mutex<Session>>,
    cache_path: PathBuf,
    cancel: Arc<AtomicBool>,
    encoding: &'static Encoding,
    generation: u64,
}

impl EncodingChange {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

struct LineScanner {
    encoding: &'static Encoding,
    pending_utf16_byte: Option<u8>,
}

impl LineScanner {
    fn new(encoding: &'static Encoding) -> Self {
        Self {
            encoding,
            pending_utf16_byte: None,
        }
    }

    fn scan(&mut self, bytes: &[u8], base: u64) -> Vec<u64> {
        if self.encoding != encoding_rs::UTF_16LE && self.encoding != encoding_rs::UTF_16BE {
            return bytes
                .iter()
                .enumerate()
                .filter_map(|(index, byte)| (*byte == b'\n').then_some(base + index as u64 + 1))
                .collect();
        }

        let mut offsets = Vec::new();
        for (index, byte) in bytes.iter().copied().enumerate() {
            if let Some(first) = self.pending_utf16_byte.take() {
                let code_unit = if self.encoding == encoding_rs::UTF_16LE {
                    u16::from_le_bytes([first, byte])
                } else {
                    u16::from_be_bytes([first, byte])
                };
                if code_unit == b'\n' as u16 {
                    offsets.push(base + index as u64 + 1);
                }
            } else {
                self.pending_utf16_byte = Some(byte);
            }
        }
        offsets
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.cache_path);
    }
}

#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
    lru: Mutex<Vec<String>>,
    cache_dir: Mutex<Option<PathBuf>>,
}

impl SessionManager {
    pub fn set_cache_dir(&self, dir: PathBuf) {
        let _ = std::fs::create_dir_all(&dir);
        *self.cache_dir.lock().unwrap() = Some(dir);
    }

    fn new_cache_path(&self, session_id: &str) -> PathBuf {
        let dir = self
            .cache_dir
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        dir.join(format!("logcrate-{session_id}.cache"))
    }

    fn detect_encoding(sample: &[u8]) -> &'static Encoding {
        if sample.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return encoding_rs::UTF_8;
        }
        if sample.starts_with(&[0xFF, 0xFE]) {
            return encoding_rs::UTF_16LE;
        }
        if sample.starts_with(&[0xFE, 0xFF]) {
            return encoding_rs::UTF_16BE;
        }
        if std::str::from_utf8(sample).is_ok() {
            encoding_rs::UTF_8
        } else {
            encoding_rs::GB18030
        }
    }

    fn encoding_by_name(name: &str) -> anyhow::Result<&'static Encoding> {
        match name.trim().to_ascii_uppercase().as_str() {
            "UTF-8" | "UTF8" => Ok(encoding_rs::UTF_8),
            "GBK" => Ok(encoding_rs::GBK),
            "GB18030" => Ok(encoding_rs::GB18030),
            "UTF-16LE" | "UTF16LE" => Ok(encoding_rs::UTF_16LE),
            "UTF-16BE" | "UTF16BE" => Ok(encoding_rs::UTF_16BE),
            _ => anyhow::bail!("unsupported encoding: {name}"),
        }
    }

    fn encoding_name(encoding: &'static Encoding) -> String {
        if encoding == encoding_rs::UTF_8 {
            "UTF-8"
        } else if encoding == encoding_rs::GBK {
            "GBK"
        } else if encoding == encoding_rs::GB18030 {
            "GB18030"
        } else if encoding == encoding_rs::UTF_16LE {
            "UTF-16LE"
        } else if encoding == encoding_rs::UTF_16BE {
            "UTF-16BE"
        } else {
            encoding.name()
        }
        .to_string()
    }

    fn append_index_chunk(
        session: &Arc<Mutex<Session>>,
        cache_io: &Arc<Mutex<()>>,
        output: &mut BufWriter<File>,
        scanner: &mut LineScanner,
        bytes: &[u8],
        written: &mut u64,
        max_uncompressed: u64,
    ) -> anyhow::Result<u64> {
        let next_written = written.saturating_add(bytes.len() as u64);
        if next_written > max_uncompressed {
            anyhow::bail!("uncompressed content exceeds the size limit");
        }
        let new_offsets = scanner.scan(bytes, *written);
        {
            let _cache_guard = cache_io.lock().unwrap();
            output.write_all(bytes)?;
            // A reader or exporter must never observe a boundary before its bytes are stable.
            output.flush()?;
        }
        *written = next_written;
        let mut current = session.lock().unwrap();
        current.offsets.extend(new_offsets);
        Ok(current.line_count())
    }

    fn trim_line_bytes(raw: &mut Vec<u8>, encoding: &'static Encoding, first_line: bool) {
        if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
            while raw.len() >= 2 {
                let pair = [raw[raw.len() - 2], raw[raw.len() - 1]];
                let code_unit = if encoding == encoding_rs::UTF_16LE {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                };
                if code_unit != b'\n' as u16 && code_unit != b'\r' as u16 {
                    break;
                }
                raw.truncate(raw.len() - 2);
            }
            if first_line
                && ((encoding == encoding_rs::UTF_16LE && raw.starts_with(&[0xFF, 0xFE]))
                    || (encoding == encoding_rs::UTF_16BE && raw.starts_with(&[0xFE, 0xFF])))
            {
                raw.drain(..2);
            }
        } else {
            while matches!(raw.last(), Some(b'\n') | Some(b'\r')) {
                raw.pop();
            }
            if first_line && raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
                raw.drain(..3);
            }
        }
    }

    /// Create an empty session so the command can return before indexing begins.
    pub fn prepare(&self, entry_path: String, declared_size: u64) -> anyhow::Result<OpenResult> {
        if declared_size > MAX_UNCOMPRESSED {
            anyhow::bail!("file is too large; files over 2GB are not supported");
        }
        let session_id = format!("s{}", SESSION_SEQ.fetch_add(1, Ordering::SeqCst));
        let cache_path = self.new_cache_path(&session_id);
        File::create(&cache_path)?;
        let session = Session {
            cache_path,
            cache_io: Arc::new(Mutex::new(())),
            offsets: vec![0],
            detected_encoding: None,
            effective_encoding: encoding_rs::UTF_8,
            indexing: true,
            encoding_generation: 0,
            field_generation: 0,
            field_state: None,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), Arc::new(Mutex::new(session)));
        let evicted_session_ids = self.touch_lru(&session_id);

        Ok(OpenResult {
            session_id,
            entry_path,
            size: declared_size,
            indexing: true,
            encoding: "Detecting".to_string(),
            evicted_session_ids,
        })
    }

    /// Fill a prepared session. Flushed bytes and their offsets are published atomically.
    #[cfg(test)]
    pub fn index<R, F>(&self, session_id: &str, declared_size: u64, reader: R, progress: F)
    where
        R: Read,
        F: FnMut(IndexProgress),
    {
        self.index_with_limit(
            session_id,
            declared_size,
            reader,
            MAX_UNCOMPRESSED,
            progress,
        );
    }

    pub(crate) fn index_with_limit<R, F>(
        &self,
        session_id: &str,
        declared_size: u64,
        mut reader: R,
        max_uncompressed: u64,
        mut progress: F,
    ) where
        R: Read,
        F: FnMut(IndexProgress),
    {
        let session = {
            let map = self.sessions.lock().unwrap();
            map.get(session_id).cloned()
        };
        let Some(session) = session else { return };
        let (cache_path, cache_io, cancel) = {
            let session = session.lock().unwrap();
            (
                session.cache_path.clone(),
                session.cache_io.clone(),
                session.cancel.clone(),
            )
        };

        let result = (|| -> anyhow::Result<u64> {
            let mut out = {
                let _cache_guard = cache_io.lock().unwrap();
                BufWriter::new(File::create(&cache_path)?)
            };
            let mut written = 0u64;
            let mut sample = [0u8; 4096];
            let sample_len = reader.read(&mut sample)?;
            let encoding = Self::detect_encoding(&sample[..sample_len]);
            let encoding_name = Self::encoding_name(encoding);
            {
                let mut current = session.lock().unwrap();
                current.detected_encoding = Some(encoding);
                current.effective_encoding = encoding;
            }
            let mut scanner = LineScanner::new(encoding);
            let mut buf = [0u8; 64 * 1024];
            let mut last_emit: Option<Instant> = None;

            if sample_len > 0 {
                let indexed_lines = Self::append_index_chunk(
                    &session,
                    &cache_io,
                    &mut out,
                    &mut scanner,
                    &sample[..sample_len],
                    &mut written,
                    max_uncompressed,
                )?;
                progress(IndexProgress {
                    session_id: session_id.to_string(),
                    percent: written
                        .saturating_mul(100)
                        .checked_div(declared_size)
                        .unwrap_or(0)
                        .min(99) as u8,
                    indexed_lines,
                    done: false,
                    failed: false,
                    detected_encoding: encoding_name.clone(),
                    effective_encoding: encoding_name.clone(),
                    error: None,
                });
                last_emit = Some(Instant::now());
            }

            loop {
                if cancel.load(Ordering::Acquire) {
                    anyhow::bail!("indexing cancelled");
                }
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let indexed_lines = Self::append_index_chunk(
                    &session,
                    &cache_io,
                    &mut out,
                    &mut scanner,
                    &buf[..n],
                    &mut written,
                    max_uncompressed,
                )?;
                let percent = written
                    .saturating_mul(100)
                    .checked_div(declared_size)
                    .unwrap_or(0)
                    .min(99) as u8;
                let now = Instant::now();
                if last_emit
                    .map(|last| now.duration_since(last) >= Duration::from_millis(50))
                    .unwrap_or(true)
                {
                    progress(IndexProgress {
                        session_id: session_id.to_string(),
                        percent,
                        indexed_lines,
                        done: false,
                        failed: false,
                        detected_encoding: encoding_name.clone(),
                        effective_encoding: encoding_name.clone(),
                        error: None,
                    });
                    last_emit = Some(now);
                }
            }

            {
                let _cache_guard = cache_io.lock().unwrap();
                out.flush()?;
            }
            let indexed_lines = {
                let mut current = session.lock().unwrap();
                if *current.offsets.last().unwrap() != written {
                    current.offsets.push(written);
                }
                current.indexing = false;
                current.line_count()
            };
            Ok(indexed_lines)
        })();

        match result {
            Ok(indexed_lines) => {
                let current = session.lock().unwrap();
                let detected = current.detected_encoding.unwrap_or(encoding_rs::UTF_8);
                progress(IndexProgress {
                    session_id: session_id.to_string(),
                    percent: 100,
                    indexed_lines,
                    done: true,
                    failed: false,
                    detected_encoding: Self::encoding_name(detected),
                    effective_encoding: Self::encoding_name(current.effective_encoding),
                    error: None,
                });
            }
            Err(_) if cancel.load(Ordering::Acquire) => {}
            Err(error) => {
                let mut current = session.lock().unwrap();
                current.indexing = false;
                let detected = current.detected_encoding.unwrap_or(encoding_rs::UTF_8);
                progress(IndexProgress {
                    session_id: session_id.to_string(),
                    percent: 100,
                    indexed_lines: current.line_count(),
                    done: true,
                    failed: true,
                    detected_encoding: Self::encoding_name(detected),
                    effective_encoding: Self::encoding_name(current.effective_encoding),
                    error: Some(error.to_string()),
                });
            }
        }
    }

    pub fn prepare_encoding_change(
        &self,
        session_id: &str,
        encoding_name: &str,
    ) -> anyhow::Result<EncodingChange> {
        let encoding = Self::encoding_by_name(encoding_name)?;
        let session = {
            let sessions = self.sessions.lock().unwrap();
            sessions.get(session_id).cloned()
        }
        .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let _ = self.touch_lru(session_id);
        let mut current = session.lock().unwrap();
        if current.indexing {
            anyhow::bail!("wait for initial indexing to finish before changing encoding");
        }
        current.encoding_generation = current.encoding_generation.saturating_add(1);
        current.field_generation = current.field_generation.saturating_add(1);
        current.field_state = None;
        let change = EncodingChange {
            session_id: session_id.to_string(),
            session: session.clone(),
            cache_path: current.cache_path.clone(),
            cancel: current.cancel.clone(),
            encoding,
            generation: current.encoding_generation,
        };
        drop(current);
        Ok(change)
    }

    pub fn apply_encoding_change<F>(&self, change: EncodingChange, mut progress: F)
    where
        F: FnMut(EncodingProgress),
    {
        let encoding_name = Self::encoding_name(change.encoding);
        let result = (|| -> anyhow::Result<Option<u64>> {
            let mut file = File::open(&change.cache_path)?;
            let total_bytes = file.metadata()?.len();
            let mut offsets = vec![0u64];
            let mut scanner = LineScanner::new(change.encoding);
            let mut buffer = [0u8; 64 * 1024];
            let mut read_bytes = 0u64;
            let mut last_emit: Option<Instant> = None;

            loop {
                if change.cancel.load(Ordering::Acquire) {
                    return Ok(None);
                }
                if change.session.lock().unwrap().encoding_generation != change.generation {
                    return Ok(None);
                }
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                offsets.extend(scanner.scan(&buffer[..count], read_bytes));
                read_bytes += count as u64;
                let now = Instant::now();
                if last_emit
                    .map(|last| now.duration_since(last) >= Duration::from_millis(50))
                    .unwrap_or(true)
                {
                    progress(EncodingProgress {
                        session_id: change.session_id.clone(),
                        generation: change.generation,
                        percent: read_bytes
                            .saturating_mul(100)
                            .checked_div(total_bytes)
                            .unwrap_or(0)
                            .min(99) as u8,
                        encoding: encoding_name.clone(),
                        line_count: offsets.len().saturating_sub(1) as u64,
                        done: false,
                        failed: false,
                        error: None,
                    });
                    last_emit = Some(now);
                }
            }

            if *offsets.last().unwrap() != total_bytes {
                offsets.push(total_bytes);
            }
            let mut current = change.session.lock().unwrap();
            if current.encoding_generation != change.generation
                || change.cancel.load(Ordering::Acquire)
            {
                return Ok(None);
            }
            current.effective_encoding = change.encoding;
            current.offsets = offsets;
            Ok(Some(current.line_count()))
        })();

        match result {
            Ok(Some(line_count)) => progress(EncodingProgress {
                session_id: change.session_id,
                generation: change.generation,
                percent: 100,
                encoding: encoding_name,
                line_count,
                done: true,
                failed: false,
                error: None,
            }),
            Ok(None) => {}
            Err(error) => {
                if change.session.lock().unwrap().encoding_generation == change.generation {
                    progress(EncodingProgress {
                        session_id: change.session_id,
                        generation: change.generation,
                        percent: 100,
                        encoding: encoding_name,
                        line_count: 0,
                        done: true,
                        failed: true,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
    }

    fn touch_lru(&self, session_id: &str) -> Vec<String> {
        let mut lru = self.lru.lock().unwrap();
        lru.retain(|s| s != session_id);
        lru.push(session_id.to_string());
        let mut evicted_session_ids = Vec::new();
        while lru.len() > MAX_SESSIONS {
            let evict = lru.remove(0);
            let removed = self.sessions.lock().unwrap().remove(&evict);
            if let Some(session) = removed {
                session
                    .lock()
                    .unwrap()
                    .cancel
                    .store(true, Ordering::Release);
                evicted_session_ids.push(evict);
            }
        }
        evicted_session_ids
    }

    pub fn read_lines(
        &self,
        session_id: &str,
        start: u64,
        count: u64,
    ) -> anyhow::Result<Vec<LogLine>> {
        let session = {
            let map = self.sessions.lock().unwrap();
            map.get(session_id).cloned()
        };
        let session = session.ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let _ = self.touch_lru(session_id);
        let session = session.lock().unwrap();

        let total = session.line_count();
        if start >= total {
            return Ok(vec![]);
        }
        let end = start.saturating_add(count).min(total);
        let mut file = File::open(&session.cache_path)?;
        let mut lines = Vec::with_capacity((end - start) as usize);
        for line_no in start..end {
            let from = session.offsets[line_no as usize];
            let to = session.offsets[line_no as usize + 1];
            let len = (to - from) as usize;
            let read_len = len.min(MAX_LINE_BYTES);
            let mut raw = vec![0u8; read_len];
            file.seek(SeekFrom::Start(from))?;
            file.read_exact(&mut raw)?;
            Self::trim_line_bytes(&mut raw, session.effective_encoding, line_no == 0);
            let (text, _, _) = session.effective_encoding.decode(&raw);
            lines.push(LogLine {
                line_no: line_no + 1,
                content: text.into_owned(),
                truncated: len > MAX_LINE_BYTES,
            });
        }
        Ok(lines)
    }

    pub fn prepare_log_field_filter(
        &self,
        session_id: &str,
        request: LogFieldFilterRequest,
    ) -> anyhow::Result<LogFieldBuild> {
        if request.layout.fields.is_empty() {
            anyhow::bail!("field layout cannot be empty");
        }
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let _ = self.touch_lru(session_id);
        let (generation, total_lines) = {
            let mut current = session.lock().unwrap();
            current.field_generation = current.field_generation.saturating_add(1);
            let generation = current.field_generation;
            let total_lines = current.line_count();
            current.field_state = Some(LogFieldSessionState {
                status: LogFieldStatus {
                    generation,
                    layout: request.layout.clone(),
                    conditions: request.conditions.clone(),
                    statistics: Vec::new(),
                    scanned_lines: 0,
                    matched_lines: 0,
                    unparsed_lines: 0,
                    total_lines,
                    done: false,
                    failed: false,
                    error: None,
                },
                matched_original_lines: Vec::new(),
                unparsed_original_lines: Vec::new(),
            });
            (generation, total_lines)
        };
        Ok(LogFieldBuild {
            session_id: session_id.to_string(),
            session,
            generation,
            request,
            total_lines,
            initial: None,
        })
    }

    pub fn analyze_log_field_layout(
        &self,
        session_id: &str,
        phase: SamplingPhase,
    ) -> anyhow::Result<LayoutAnalysis> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let (total, cache_path) = {
            let current = session.lock().unwrap();
            (current.line_count(), current.cache_path.clone())
        };
        let mut file = File::open(cache_path)?;
        Ok(analyze_layout(total as usize, phase, |index| {
            Self::read_field_line_from(&mut file, &session, index as u64)
                .ok()
                .and_then(|(line, truncated)| (!truncated).then_some(line))
        }))
    }

    /// Continue the current generation when the line index has published more complete lines.
    /// A running or already current build is left untouched.
    pub fn prepare_log_field_refresh(&self, session_id: &str) -> Option<LogFieldBuild> {
        let session = self.sessions.lock().unwrap().get(session_id).cloned()?;
        let mut current = session.lock().unwrap();
        let total_lines = current.line_count();
        let state = current.field_state.as_mut()?;
        if !state.status.done || state.status.failed || state.status.scanned_lines >= total_lines {
            return None;
        }
        state.status.done = false;
        state.status.total_lines = total_lines;
        let initial = LogFieldScanResult {
            matched_lines: state.matched_original_lines.clone(),
            unparsed_lines: state.unparsed_original_lines.clone(),
            statistics: state.status.statistics.clone(),
            scanned_lines: state.status.scanned_lines,
        };
        let request = LogFieldFilterRequest {
            layout: state.status.layout.clone(),
            conditions: state.status.conditions.clone(),
        };
        let build = LogFieldBuild {
            session_id: session_id.to_string(),
            session: session.clone(),
            generation: state.status.generation,
            request,
            total_lines,
            initial: Some(initial),
        };
        drop(current);
        Some(build)
    }

    fn read_field_line_from(
        file: &mut File,
        session: &Arc<Mutex<Session>>,
        line_index: u64,
    ) -> anyhow::Result<(String, bool)> {
        let (encoding, from, to) = {
            let current = session.lock().unwrap();
            if line_index >= current.line_count() {
                anyhow::bail!("line index is no longer available");
            }
            (
                current.effective_encoding,
                current.offsets[line_index as usize],
                current.offsets[line_index as usize + 1],
            )
        };
        let len = (to - from) as usize;
        let read_len = len.min(MAX_LINE_BYTES);
        let mut raw = vec![0u8; read_len];
        file.seek(SeekFrom::Start(from))?;
        file.read_exact(&mut raw)?;
        Self::trim_line_bytes(&mut raw, encoding, line_index == 0);
        let (text, _, _) = encoding.decode(&raw);
        Ok((text.into_owned(), len > MAX_LINE_BYTES))
    }

    pub fn apply_log_field_filter<F>(&self, build: LogFieldBuild, mut progress: F)
    where
        F: FnMut(LogFieldProgress),
    {
        let session_id = build.session_id.clone();
        let generation = build.generation;
        let total_lines = build.total_lines;
        let session = build.session.clone();
        let mut published_matches = build
            .initial
            .as_ref()
            .map_or(0, |value| value.matched_lines.len());
        let mut published_unparsed = build
            .initial
            .as_ref()
            .map_or(0, |value| value.unparsed_lines.len());
        let result = (|| -> anyhow::Result<Option<LogFieldScanResult>> {
            let cache_path = session.lock().unwrap().cache_path.clone();
            let mut file = File::open(cache_path)?;
            let cancelled = || {
                let current = session.lock().unwrap();
                current.cancel.load(Ordering::Acquire) || current.field_generation != generation
            };
            if build.initial.is_some() {
                extend_field_lines(
                    total_lines,
                    &build.request.layout,
                    &build.request.conditions,
                    build.initial,
                    |line| Self::read_field_line_from(&mut file, &session, line),
                    cancelled,
                    |scanned, matched, unparsed| {
                        let mut current = session.lock().unwrap();
                        if current.field_generation != generation {
                            return;
                        }
                        if let Some(state) = current.field_state.as_mut() {
                            state
                                .matched_original_lines
                                .extend_from_slice(&matched[published_matches..]);
                            state
                                .unparsed_original_lines
                                .extend_from_slice(&unparsed[published_unparsed..]);
                            published_matches = matched.len();
                            published_unparsed = unparsed.len();
                            state.status.scanned_lines = scanned;
                            state.status.matched_lines = matched.len() as u64;
                            state.status.unparsed_lines = unparsed.len() as u64;
                        }
                        drop(current);
                        progress(LogFieldProgress {
                            session_id: session_id.clone(),
                            generation,
                            scanned_lines: scanned,
                            matched_lines: matched.len() as u64,
                            unparsed_lines: unparsed.len() as u64,
                            total_lines,
                            done: false,
                            failed: false,
                            error: None,
                        });
                    },
                )
            } else {
                scan_field_lines(
                    total_lines,
                    &build.request.layout,
                    &build.request.conditions,
                    |line| Self::read_field_line_from(&mut file, &session, line),
                    cancelled,
                    |scanned, matched, unparsed| {
                        let mut current = session.lock().unwrap();
                        if current.field_generation != generation {
                            return;
                        }
                        if let Some(state) = current.field_state.as_mut() {
                            state
                                .matched_original_lines
                                .extend_from_slice(&matched[published_matches..]);
                            state
                                .unparsed_original_lines
                                .extend_from_slice(&unparsed[published_unparsed..]);
                            published_matches = matched.len();
                            published_unparsed = unparsed.len();
                            state.status.scanned_lines = scanned;
                            state.status.matched_lines = matched.len() as u64;
                            state.status.unparsed_lines = unparsed.len() as u64;
                        }
                        drop(current);
                        progress(LogFieldProgress {
                            session_id: session_id.clone(),
                            generation,
                            scanned_lines: scanned,
                            matched_lines: matched.len() as u64,
                            unparsed_lines: unparsed.len() as u64,
                            total_lines,
                            done: false,
                            failed: false,
                            error: None,
                        });
                    },
                )
            }
        })();
        match result {
            Ok(Some(result)) => {
                let mut current = session.lock().unwrap();
                if current.field_generation != generation {
                    return;
                }
                if let Some(state) = current.field_state.as_mut() {
                    state.matched_original_lines = result.matched_lines;
                    state.unparsed_original_lines = result.unparsed_lines;
                    state.status.statistics = result.statistics;
                    state.status.scanned_lines = result.scanned_lines;
                    state.status.matched_lines = state.matched_original_lines.len() as u64;
                    state.status.unparsed_lines = state.unparsed_original_lines.len() as u64;
                    state.status.done = true;
                }
                let state = current.field_state.as_ref().unwrap();
                let event = LogFieldProgress {
                    session_id,
                    generation,
                    scanned_lines: state.status.scanned_lines,
                    matched_lines: state.status.matched_lines,
                    unparsed_lines: state.status.unparsed_lines,
                    total_lines,
                    done: true,
                    failed: false,
                    error: None,
                };
                drop(current);
                progress(event);
            }
            Ok(None) => {}
            Err(error) => {
                let mut current = session.lock().unwrap();
                if current.field_generation != generation {
                    return;
                }
                let scanned_lines = if let Some(state) = current.field_state.as_mut() {
                    state.matched_original_lines.clear();
                    state.unparsed_original_lines.clear();
                    state.status.done = true;
                    state.status.failed = true;
                    state.status.error = Some(error.to_string());
                    state.status.matched_lines = 0;
                    state.status.unparsed_lines = 0;
                    state.status.scanned_lines
                } else {
                    0
                };
                drop(current);
                progress(LogFieldProgress {
                    session_id,
                    generation,
                    scanned_lines,
                    matched_lines: 0,
                    unparsed_lines: 0,
                    total_lines,
                    done: true,
                    failed: true,
                    error: Some(error.to_string()),
                });
            }
        }
    }

    pub fn log_field_status(&self, session_id: &str) -> Option<LogFieldStatus> {
        self.sessions
            .lock()
            .unwrap()
            .get(session_id)
            .and_then(|session| session.lock().unwrap().field_state.as_ref().cloned())
            .map(|state| state.status)
    }

    pub fn clear_log_field_filter(&self, session_id: &str) -> anyhow::Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let mut current = session.lock().unwrap();
        current.field_generation = current.field_generation.saturating_add(1);
        current.field_state = None;
        Ok(())
    }

    fn compact_view_lines(state: &LogFieldSessionState, include_unparsed: bool) -> Vec<u64> {
        if !include_unparsed {
            return state.matched_original_lines.clone();
        }
        let mut lines = Vec::with_capacity(
            state.matched_original_lines.len() + state.unparsed_original_lines.len(),
        );
        let (mut matched, mut unparsed) = (0usize, 0usize);
        while matched < state.matched_original_lines.len()
            || unparsed < state.unparsed_original_lines.len()
        {
            let next_match = state.matched_original_lines.get(matched).copied();
            let next_unparsed = state.unparsed_original_lines.get(unparsed).copied();
            match (next_match, next_unparsed) {
                (Some(left), Some(right)) if left <= right => {
                    lines.push(left);
                    matched += 1;
                }
                (_, Some(right)) => {
                    lines.push(right);
                    unparsed += 1;
                }
                (Some(left), None) => {
                    lines.push(left);
                    matched += 1;
                }
                (None, None) => break,
            }
        }
        lines
    }

    fn field_state_for_generation(
        &self,
        session_id: &str,
        generation: u64,
    ) -> anyhow::Result<Arc<Mutex<Session>>> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        {
            let current = session.lock().unwrap();
            current
                .field_state
                .as_ref()
                .filter(|state| state.status.generation == generation && !state.status.failed)
                .ok_or_else(|| anyhow::anyhow!("field generation is not available"))?;
        }
        Ok(session)
    }

    pub fn read_filtered_lines(
        &self,
        session_id: &str,
        generation: u64,
        start: u64,
        count: u64,
        include_unparsed: bool,
    ) -> anyhow::Result<Vec<LogLine>> {
        let session = self.field_state_for_generation(session_id, generation)?;
        let (cache_path, originals) = {
            let current = session.lock().unwrap();
            let state = current.field_state.as_ref().unwrap();
            let take = count as usize;
            let originals = if include_unparsed {
                let mut merged = Vec::with_capacity(take);
                let (mut left, mut right, mut skipped) = (0usize, 0usize, 0u64);
                while merged.len() < take
                    && (left < state.matched_original_lines.len()
                        || right < state.unparsed_original_lines.len())
                {
                    let matched = state.matched_original_lines.get(left).copied();
                    let unparsed = state.unparsed_original_lines.get(right).copied();
                    let next = match (matched, unparsed) {
                        (Some(a), Some(b)) if a <= b => {
                            left += 1;
                            a
                        }
                        (_, Some(b)) => {
                            right += 1;
                            b
                        }
                        (Some(a), None) => {
                            left += 1;
                            a
                        }
                        (None, None) => break,
                    };
                    if skipped < start {
                        skipped += 1;
                    } else {
                        merged.push(next);
                    }
                }
                merged
            } else {
                let from = (start as usize).min(state.matched_original_lines.len());
                let to = from
                    .saturating_add(take)
                    .min(state.matched_original_lines.len());
                state.matched_original_lines[from..to].to_vec()
            };
            (current.cache_path.clone(), originals)
        };
        let mut file = File::open(cache_path)?;
        let mut lines = Vec::with_capacity(originals.len());
        for original in originals {
            let (content, truncated) = Self::read_field_line_from(&mut file, &session, original)?;
            lines.push(LogLine {
                line_no: original + 1,
                content,
                truncated,
            });
        }
        Ok(lines)
    }

    pub fn read_lines_with_field_matches(
        &self,
        session_id: &str,
        generation: u64,
        start: u64,
        count: u64,
    ) -> anyhow::Result<Vec<LogFieldMarkedLine>> {
        let session = self.field_state_for_generation(session_id, generation)?;
        let lines = self.read_lines(session_id, start, count)?;
        let current = session.lock().unwrap();
        let state = current.field_state.as_ref().unwrap();
        Ok(lines
            .into_iter()
            .map(|line| {
                let original = line.line_no - 1;
                LogFieldMarkedLine {
                    field_matched: state
                        .matched_original_lines
                        .binary_search(&original)
                        .is_ok(),
                    field_unparsed: state
                        .unparsed_original_lines
                        .binary_search(&original)
                        .is_ok(),
                    line,
                }
            })
            .collect())
    }

    pub fn locate_log_field_anchor(
        &self,
        session_id: &str,
        generation: u64,
        original_line_no: u64,
        mode: LogFieldResultMode,
        include_unparsed: bool,
    ) -> anyhow::Result<Option<LogFieldAnchorResult>> {
        let session = self.field_state_for_generation(session_id, generation)?;
        let total = self.line_count(session_id);
        if total == 0 {
            return Ok(None);
        }
        let target = original_line_no.saturating_sub(1).min(total - 1);
        if matches!(mode, LogFieldResultMode::Highlight) {
            return Ok(Some(LogFieldAnchorResult {
                view_index: target,
                line_no: target + 1,
            }));
        }
        let current = session.lock().unwrap();
        let state = current.field_state.as_ref().unwrap();
        let next_in = |lines: &[u64]| match lines.binary_search(&target) {
            Ok(index) => lines.get(index).copied(),
            Err(index) => lines.get(index).copied(),
        };
        let previous_in = |lines: &[u64]| match lines.binary_search(&target) {
            Ok(index) => lines.get(index).copied(),
            Err(0) => None,
            Err(index) => lines.get(index - 1).copied(),
        };
        let matched_next = next_in(&state.matched_original_lines);
        let unparsed_next = include_unparsed
            .then(|| next_in(&state.unparsed_original_lines))
            .flatten();
        let selected = match (matched_next, unparsed_next) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        }
        .or_else(|| {
            let matched = previous_in(&state.matched_original_lines);
            let unparsed = include_unparsed
                .then(|| previous_in(&state.unparsed_original_lines))
                .flatten();
            match (matched, unparsed) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (left, right) => left.or(right),
            }
        });
        let Some(selected) = selected else {
            return Ok(None);
        };
        let matched_before = state
            .matched_original_lines
            .partition_point(|line| *line < selected);
        let unparsed_before = if include_unparsed {
            state
                .unparsed_original_lines
                .partition_point(|line| *line < selected)
        } else {
            0
        };
        Ok(Some(LogFieldAnchorResult {
            view_index: (matched_before + unparsed_before) as u64,
            line_no: selected + 1,
        }))
    }

    fn is_word_character(character: char) -> bool {
        character.is_alphanumeric() || character == '_'
    }

    fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        before.map_or(true, |character| !Self::is_word_character(character))
            && after.map_or(true, |character| !Self::is_word_character(character))
    }

    fn folded_text_with_source_ranges(text: &str) -> (String, Vec<(usize, usize)>) {
        let mut folded = String::new();
        let mut ranges = Vec::new();
        for (start, character) in text.char_indices() {
            let end = start + character.len_utf8();
            for folded_character in character.to_lowercase() {
                folded.push(folded_character);
                ranges.push((start, end));
            }
        }
        (folded, ranges)
    }

    fn match_byte_ranges(text: &str, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
        if case_sensitive {
            return text
                .match_indices(query)
                .map(|(start, matched)| (start, start + matched.len()))
                .collect();
        }

        let (folded_text, source_ranges) = Self::folded_text_with_source_ranges(text);
        let folded_query: String = query.chars().flat_map(char::to_lowercase).collect();
        if folded_query.is_empty() {
            return Vec::new();
        }
        folded_text
            .match_indices(&folded_query)
            .filter_map(|(folded_start, matched)| {
                let start_index = folded_text[..folded_start].chars().count();
                let end_index = start_index + matched.chars().count();
                let &(source_start, _) = source_ranges.get(start_index)?;
                let &(_, source_end) = source_ranges.get(end_index.checked_sub(1)?)?;
                let begins_source_character = start_index == 0
                    || source_ranges[start_index - 1].0 != source_ranges[start_index].0;
                let ends_source_character = end_index == source_ranges.len()
                    || source_ranges[end_index - 1].0 != source_ranges[end_index].0;
                (begins_source_character && ends_source_character)
                    .then_some((source_start, source_end))
            })
            .collect()
    }

    fn utf16_column(text: &str, byte_index: usize) -> u64 {
        text[..byte_index].encode_utf16().count() as u64
    }

    fn matches_in_line(text: &str, request: &LogSearchRequest) -> Vec<(u64, u64)> {
        Self::match_byte_ranges(text, &request.query, request.case_sensitive)
            .into_iter()
            .filter(|(start, end)| !request.whole_word || Self::is_whole_word(text, *start, *end))
            .map(|(start, end)| {
                (
                    Self::utf16_column(text, start),
                    Self::utf16_column(text, end),
                )
            })
            .collect()
    }

    fn read_search_line(
        file: &mut File,
        session: &Arc<Mutex<Session>>,
        line_no: u64,
        encoding: &'static Encoding,
    ) -> anyhow::Result<String> {
        let (from, to) = {
            let current = session.lock().unwrap();
            (
                current.offsets[line_no as usize],
                current.offsets[line_no as usize + 1],
            )
        };
        let read_len = (to - from).min(MAX_LINE_BYTES as u64) as usize;
        let mut raw = vec![0u8; read_len];
        file.seek(SeekFrom::Start(from))?;
        file.read_exact(&mut raw)?;
        Self::trim_line_bytes(&mut raw, encoding, line_no == 0);
        let (text, _, _) = encoding.decode(&raw);
        Ok(text.into_owned())
    }

    pub fn search_log(
        &self,
        session_id: &str,
        request: &LogSearchRequest,
    ) -> anyhow::Result<LogSearchResult> {
        if request.query.is_empty() {
            anyhow::bail!("search query cannot be empty");
        }
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let _ = self.touch_lru(session_id);
        let (cache_path, encoding, indexed_lines, indexing) = {
            let current = session.lock().unwrap();
            (
                current.cache_path.clone(),
                current.effective_encoding,
                current.line_count(),
                current.indexing,
            )
        };
        let allowed_lines = match request.field_view.as_ref() {
            Some(view) if matches!(view.mode, LogFieldResultMode::Compact) => {
                let field_session = self.field_state_for_generation(session_id, view.generation)?;
                let current = field_session.lock().unwrap();
                Some(Self::compact_view_lines(
                    current.field_state.as_ref().unwrap(),
                    view.include_unparsed,
                ))
            }
            _ => None,
        };
        if indexed_lines == 0 {
            return Ok(LogSearchResult {
                matched: None,
                wrapped: false,
                reached_boundary: true,
                indexed_lines,
                indexing,
            });
        }

        let start_line = request.start_line.min(indexed_lines - 1);
        let start_column = request.start_column;
        let mut file = File::open(cache_path)?;
        let find_forward = |file: &mut File,
                            from_line: u64,
                            to_line: u64,
                            first_min: Option<u64>,
                            last_max: Option<u64>|
         -> anyhow::Result<Option<LogSearchMatch>> {
            for line in from_line..to_line {
                if matches!(
                    allowed_lines.as_ref(),
                    Some(lines) if lines.binary_search(&line).is_err()
                ) {
                    continue;
                }
                let text = Self::read_search_line(file, &session, line, encoding)?;
                let minimum = (line == from_line)
                    .then_some(first_min)
                    .flatten()
                    .unwrap_or(0);
                let maximum = (line + 1 == to_line).then_some(last_max).flatten();
                if let Some((start, end)) =
                    Self::matches_in_line(&text, request)
                        .into_iter()
                        .find(|(start, _)| {
                            *start >= minimum && maximum.map_or(true, |max| *start < max)
                        })
                {
                    return Ok(Some(LogSearchMatch {
                        line_no: line + 1,
                        start_column: start,
                        end_column: end,
                    }));
                }
            }
            Ok(None)
        };
        let find_reverse = |file: &mut File,
                            from_line: u64,
                            to_line_inclusive: u64,
                            first_max: Option<u64>,
                            last_min: Option<u64>|
         -> anyhow::Result<Option<LogSearchMatch>> {
            for line in (to_line_inclusive..=from_line).rev() {
                if matches!(
                    allowed_lines.as_ref(),
                    Some(lines) if lines.binary_search(&line).is_err()
                ) {
                    continue;
                }
                let text = Self::read_search_line(file, &session, line, encoding)?;
                let maximum = (line == from_line).then_some(first_max).flatten();
                let minimum = (line == to_line_inclusive)
                    .then_some(last_min)
                    .flatten()
                    .unwrap_or(0);
                if let Some((start, end)) = Self::matches_in_line(&text, request)
                    .into_iter()
                    .rev()
                    .find(|(start, end)| {
                        *start >= minimum && maximum.map_or(true, |max| *end <= max)
                    })
                {
                    return Ok(Some(LogSearchMatch {
                        line_no: line + 1,
                        start_column: start,
                        end_column: end,
                    }));
                }
            }
            Ok(None)
        };

        let matched = if request.reverse {
            find_reverse(&mut file, start_line, 0, start_column, None)?
        } else {
            find_forward(
                &mut file,
                start_line,
                indexed_lines,
                Some(start_column.unwrap_or(0)),
                None,
            )?
        };
        if matched.is_some() || !request.wrap {
            let reached_boundary = matched.is_none();
            return Ok(LogSearchResult {
                matched,
                wrapped: false,
                reached_boundary,
                indexed_lines,
                indexing,
            });
        }

        let matched = if request.reverse {
            find_reverse(&mut file, indexed_lines - 1, start_line, None, start_column)?
        } else {
            find_forward(
                &mut file,
                0,
                start_line + 1,
                None,
                Some(start_column.unwrap_or(0)),
            )?
        };
        let reached_boundary = matched.is_none();
        Ok(LogSearchResult {
            matched,
            wrapped: true,
            reached_boundary,
            indexed_lines,
            indexing,
        })
    }

    pub fn line_count(&self, session_id: &str) -> u64 {
        self.sessions
            .lock()
            .unwrap()
            .get(session_id)
            .map(|session| session.lock().unwrap().line_count())
            .unwrap_or(0)
    }

    pub fn export_snapshot(
        &self,
        session_id: &str,
        destination: &Path,
    ) -> anyhow::Result<SnapshotExportResult> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        let (cache_path, cache_io) = {
            let current = session.lock().unwrap();
            (current.cache_path.clone(), current.cache_io.clone())
        };
        if destination == cache_path {
            anyhow::bail!("snapshot destination cannot be the session cache");
        }
        if destination.is_dir() {
            anyhow::bail!("snapshot destination is a directory");
        }

        let (stable_len, complete) = {
            let _cache_guard = cache_io.lock().unwrap();
            let stable_len = std::fs::metadata(&cache_path)?.len();
            let complete = !session.lock().unwrap().indexing;
            (stable_len, complete)
        };
        let source = File::open(&cache_path)?;
        let mut destination = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(destination)?;
        let bytes = std::io::copy(&mut source.take(stable_len), &mut destination)?;
        destination.flush()?;
        Ok(SnapshotExportResult { bytes, complete })
    }

    pub fn close(&self, session_id: &str) {
        let removed = self.sessions.lock().unwrap().remove(session_id);
        if let Some(session) = removed {
            session
                .lock()
                .unwrap()
                .cancel
                .store(true, Ordering::Release);
        }
        self.lru.lock().unwrap().retain(|id| id != session_id);
    }

    #[allow(dead_code)]
    pub fn clear_all(&self) {
        let sessions = std::mem::take(&mut *self.sessions.lock().unwrap());
        for session in sessions.into_values() {
            session
                .lock()
                .unwrap()
                .cancel
                .store(true, Ordering::Release);
        }
        self.lru.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::mpsc;

    static CACHE_TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn indexed_manager(bytes: Vec<u8>) -> (SessionManager, String, Vec<IndexProgress>) {
        let manager = SessionManager::default();
        let open = manager
            .prepare("encoding.log".into(), bytes.len() as u64)
            .unwrap();
        let mut events = Vec::new();
        manager.index(
            &open.session_id,
            bytes.len() as u64,
            Cursor::new(bytes),
            |event| events.push(event),
        );
        (manager, open.session_id, events)
    }

    fn snapshot_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "logcrate-snapshot-test-{}-{}-{name}",
            std::process::id(),
            CACHE_TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn utf16_bytes(text: &str, big_endian: bool, bom: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if bom {
            bytes.extend(if big_endian {
                [0xFE, 0xFF]
            } else {
                [0xFF, 0xFE]
            });
        }
        for unit in text.encode_utf16() {
            bytes.extend(if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        bytes
    }

    #[test]
    fn publishes_readable_lines_before_indexing_finishes() {
        let manager = Arc::new(SessionManager::default());
        let open = manager.prepare("test.log".into(), 12).unwrap();
        let session_id = open.session_id.clone();
        let (tx, rx) = mpsc::channel();
        let read_while_indexing = Arc::new(AtomicBool::new(false));
        let observed = read_while_indexing.clone();
        let reader_manager = manager.clone();
        let reader_session_id = session_id.clone();
        manager.index(&session_id, 12, Cursor::new(b"one\ntwo\nlast"), |event| {
            if !event.done && event.indexed_lines > 0 {
                let lines = reader_manager
                    .read_lines(&reader_session_id, 0, 200)
                    .unwrap();
                assert!(!lines.is_empty());
                observed.store(true, Ordering::Release);
            }
            tx.send(event).unwrap();
        });

        let events: Vec<_> = rx.try_iter().collect();
        assert!(events.iter().any(|event| !event.done));
        assert!(events.last().unwrap().done);
        assert!(read_while_indexing.load(Ordering::Acquire));
        assert_eq!(manager.line_count(&session_id), 3);
        let lines = manager.read_lines(&session_id, 1, 99).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].content, "two");
        assert_eq!(lines[1].content, "last");
    }

    #[test]
    fn read_lines_clamps_to_the_published_boundary() {
        let manager = SessionManager::default();
        let open = manager.prepare("partial.log".into(), 0).unwrap();
        assert!(manager
            .read_lines(&open.session_id, 0, 200)
            .unwrap()
            .is_empty());
    }

    fn search_request(query: &str) -> LogSearchRequest {
        LogSearchRequest {
            query: query.to_string(),
            start_line: 0,
            start_column: None,
            reverse: false,
            whole_word: false,
            case_sensitive: false,
            wrap: false,
            field_view: None,
        }
    }

    #[test]
    fn searches_forward_reverse_and_respects_match_options() {
        let (manager, session_id, _) = indexed_manager(
            "Error first\nerror errors error_code\nmiddle error\nlast ERROR\nÄpfel"
                .as_bytes()
                .to_vec(),
        );

        let first = manager
            .search_log(&session_id, &search_request("error"))
            .unwrap()
            .matched
            .unwrap();
        assert_eq!(first.line_no, 1);
        assert_eq!((first.start_column, first.end_column), (0, 5));

        let mut whole_word = search_request("error");
        whole_word.start_line = 1;
        whole_word.start_column = Some(1);
        whole_word.whole_word = true;
        assert_eq!(
            manager
                .search_log(&session_id, &whole_word)
                .unwrap()
                .matched
                .unwrap()
                .line_no,
            3
        );

        let mut case_sensitive = search_request("ERROR");
        case_sensitive.case_sensitive = true;
        assert_eq!(
            manager
                .search_log(&session_id, &case_sensitive)
                .unwrap()
                .matched
                .unwrap()
                .line_no,
            4
        );

        let mut unicode_case = search_request("äpfel");
        unicode_case.start_line = 4;
        assert_eq!(
            manager
                .search_log(&session_id, &unicode_case)
                .unwrap()
                .matched
                .unwrap()
                .line_no,
            5
        );

        let mut reverse = search_request("error");
        reverse.start_line = 3;
        reverse.start_column = Some(0);
        reverse.reverse = true;
        assert_eq!(
            manager
                .search_log(&session_id, &reverse)
                .unwrap()
                .matched
                .unwrap()
                .line_no,
            3
        );
    }

    #[test]
    fn search_wraps_once_or_reports_the_reached_boundary() {
        let (manager, session_id, _) = indexed_manager(b"first hit\nlast hit".to_vec());
        let mut request = search_request("hit");
        request.start_line = 1;
        request.start_column = Some(8);

        let boundary = manager.search_log(&session_id, &request).unwrap();
        assert!(boundary.matched.is_none());
        assert!(!boundary.wrapped);
        assert!(boundary.reached_boundary);

        request.wrap = true;
        let wrapped = manager.search_log(&session_id, &request).unwrap();
        assert_eq!(wrapped.matched.unwrap().line_no, 1);
        assert!(wrapped.wrapped);
        assert!(!wrapped.reached_boundary);

        request.reverse = true;
        request.start_line = 0;
        request.start_column = Some(0);
        let reverse_wrapped = manager.search_log(&session_id, &request).unwrap();
        assert_eq!(reverse_wrapped.matched.unwrap().line_no, 2);
        assert!(reverse_wrapped.wrapped);

        request.query = "missing".into();
        let missing = manager.search_log(&session_id, &request).unwrap();
        assert!(missing.matched.is_none());
        assert!(missing.wrapped);
        assert!(missing.reached_boundary);
    }

    #[test]
    fn search_reports_the_current_indexed_window_while_indexing() {
        let manager = SessionManager::default();
        let bytes = b"first match\nsecond line\nunfinished".to_vec();
        let open = manager
            .prepare("partial-search.log".into(), bytes.len() as u64)
            .unwrap();
        let mut observed = None;
        manager.index(
            &open.session_id,
            bytes.len() as u64,
            Cursor::new(bytes),
            |progress| {
                if !progress.done && progress.indexed_lines > 0 && observed.is_none() {
                    observed = Some(
                        manager
                            .search_log(&open.session_id, &search_request("match"))
                            .unwrap(),
                    );
                }
            },
        );

        let result = observed.unwrap();
        assert_eq!(result.matched.unwrap().line_no, 1);
        assert!(result.indexing);
        assert_eq!(result.indexed_lines, 2);
    }

    fn field_request(manager: &SessionManager, session_id: &str) -> LogFieldFilterRequest {
        let layout = manager
            .analyze_log_field_layout(session_id, SamplingPhase::Quick)
            .unwrap()
            .layout
            .unwrap();
        LogFieldFilterRequest {
            layout,
            conditions: vec![
                LogFieldCondition::Discrete {
                    field_id: "field-2".into(),
                    values: vec!["INFO".into()],
                },
                LogFieldCondition::Discrete {
                    field_id: "field-3".into(),
                    values: vec!["Main".into()],
                },
            ],
        }
    }

    fn build_fields(
        manager: &SessionManager,
        session_id: &str,
        request: LogFieldFilterRequest,
    ) -> u64 {
        let build = manager
            .prepare_log_field_filter(session_id, request)
            .unwrap();
        let generation = build.generation();
        manager.apply_log_field_filter(build, |_| {});
        generation
    }

    #[test]
    fn field_windows_preserve_compact_original_order_and_highlight_full_order() {
        let source = concat!(
            "[2026-06-05 10:00:00] [INFO] [Main] - first hit\n",
            "    at stack frame\n",
            "[2026-06-05 10:00:02] [WARN] [Worker] - hidden hit\n",
            "[2026-06-05 10:00:03] [ERROR] [Main] - hidden hit\n",
            "[2026-06-05 10:00:04] [info] [Main] - final hit"
        );
        let (manager, session_id, _) = indexed_manager(source.as_bytes().to_vec());
        let generation = build_fields(&manager, &session_id, field_request(&manager, &session_id));
        let compact = manager
            .read_filtered_lines(&session_id, generation, 0, 20, false)
            .unwrap();
        assert_eq!(
            compact.iter().map(|line| line.line_no).collect::<Vec<_>>(),
            vec![1, 5]
        );
        let with_unparsed = manager
            .read_filtered_lines(&session_id, generation, 0, 20, true)
            .unwrap();
        assert_eq!(
            with_unparsed
                .iter()
                .map(|line| line.line_no)
                .collect::<Vec<_>>(),
            vec![1, 2, 5]
        );
        let marked = manager
            .read_lines_with_field_matches(&session_id, generation, 0, 20)
            .unwrap();
        assert_eq!(
            marked
                .iter()
                .map(|line| line.line.line_no)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert!(marked[0].field_matched);
        assert!(marked[1].field_unparsed);
        assert!(!marked[2].field_matched);

        let next = manager
            .locate_log_field_anchor(
                &session_id,
                generation,
                3,
                LogFieldResultMode::Compact,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!((next.view_index, next.line_no), (1, 5));
        let original = manager
            .locate_log_field_anchor(
                &session_id,
                generation,
                3,
                LogFieldResultMode::Highlight,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!((original.view_index, original.line_no), (2, 3));
    }

    #[test]
    fn keyword_search_scope_follows_compact_or_highlight_field_view() {
        let source = concat!(
            "[2026-06-05 10:00:00] [INFO] [Main] - first hit\n",
            "[2026-06-05 10:00:01] [WARN] [Worker] - hidden hit\n",
            "[2026-06-05 10:00:02] [INFO] [Main] - final hit"
        );
        let (manager, session_id, _) = indexed_manager(source.as_bytes().to_vec());
        let generation = build_fields(&manager, &session_id, field_request(&manager, &session_id));
        let mut request = search_request("hit");
        request.start_line = 1;
        request.field_view = Some(LogFieldSearchView {
            generation,
            mode: LogFieldResultMode::Compact,
            include_unparsed: false,
        });
        assert_eq!(
            manager
                .search_log(&session_id, &request)
                .unwrap()
                .matched
                .unwrap()
                .line_no,
            3
        );
        request.field_view.as_mut().unwrap().mode = LogFieldResultMode::Highlight;
        assert_eq!(
            manager
                .search_log(&session_id, &request)
                .unwrap()
                .matched
                .unwrap()
                .line_no,
            2
        );
    }

    #[test]
    fn latest_generation_wins_and_encoding_or_close_releases_field_state() {
        let source =
            "[2026-06-05 10:00:00] [INFO] [Main] - one\n[2026-06-05 10:00:01] [WARN] [Main] - two";
        let (manager, session_id, _) = indexed_manager(source.as_bytes().to_vec());
        let request = field_request(&manager, &session_id);
        let stale = manager
            .prepare_log_field_filter(&session_id, request.clone())
            .unwrap();
        let current = manager
            .prepare_log_field_filter(&session_id, request)
            .unwrap();
        let current_generation = current.generation();
        let mut stale_events = Vec::new();
        manager.apply_log_field_filter(stale, |event| stale_events.push(event));
        assert!(stale_events.is_empty());
        manager.apply_log_field_filter(current, |_| {});
        assert_eq!(
            manager.log_field_status(&session_id).unwrap().generation,
            current_generation
        );

        let change = manager
            .prepare_encoding_change(&session_id, "UTF-8")
            .unwrap();
        assert!(manager.log_field_status(&session_id).is_none());
        manager.apply_encoding_change(change, |_| {});

        let request = field_request(&manager, &session_id);
        let pending = manager
            .prepare_log_field_filter(&session_id, request)
            .unwrap();
        manager.close(&session_id);
        manager.apply_log_field_filter(pending, |_| panic!("closed build emitted progress"));
        assert!(manager.log_field_status(&session_id).is_none());
    }

    #[test]
    fn field_refresh_extends_the_same_generation_after_index_growth() {
        let source = "[2026-06-05 10:00:00] [INFO] [Main] - one\n";
        let (manager, session_id, _) = indexed_manager(source.as_bytes().to_vec());
        let generation = build_fields(&manager, &session_id, field_request(&manager, &session_id));
        let session = manager
            .sessions
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap();
        let appended = b"[2026-06-05 10:00:01] [INFO] [Main] - two";
        {
            let current = session.lock().unwrap();
            let _cache_guard = current.cache_io.lock().unwrap();
            let mut file = OpenOptions::new()
                .append(true)
                .open(&current.cache_path)
                .unwrap();
            file.write_all(appended).unwrap();
            file.flush().unwrap();
        }
        {
            let mut current = session.lock().unwrap();
            let end = std::fs::metadata(&current.cache_path).unwrap().len();
            current.offsets.push(end);
        }
        let refresh = manager.prepare_log_field_refresh(&session_id).unwrap();
        assert_eq!(refresh.generation(), generation);
        manager.apply_log_field_filter(refresh, |_| {});
        let status = manager.log_field_status(&session_id).unwrap();
        assert_eq!(status.scanned_lines, 2);
        assert_eq!(status.matched_lines, 2);
    }

    #[test]
    fn field_scan_failure_falls_back_without_publishing_partial_windows() {
        let source = "[2026-06-05 10:00:00] [INFO] [Main] - one";
        let (manager, session_id, _) = indexed_manager(source.as_bytes().to_vec());
        let build = manager
            .prepare_log_field_filter(&session_id, field_request(&manager, &session_id))
            .unwrap();
        let generation = build.generation();
        let cache_path = build.session.lock().unwrap().cache_path.clone();
        std::fs::remove_file(cache_path).unwrap();
        let mut events = Vec::new();
        manager.apply_log_field_filter(build, |event| events.push(event));
        let status = manager.log_field_status(&session_id).unwrap();
        assert!(status.failed);
        assert!(events.last().unwrap().failed);
        assert!(manager
            .read_filtered_lines(&session_id, generation, 0, 10, false)
            .is_err());
    }

    #[test]
    fn exports_complete_session_bytes() {
        let bytes = b"first\nsecond\nlast".to_vec();
        let (manager, session_id, _) = indexed_manager(bytes.clone());
        let destination = snapshot_path("complete.log");

        let result = manager.export_snapshot(&session_id, &destination).unwrap();

        assert_eq!(result.bytes, bytes.len() as u64);
        assert!(result.complete);
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        std::fs::remove_file(destination).unwrap();
    }

    #[test]
    fn exports_only_the_stable_prefix_while_indexing() {
        let bytes = vec![b'x'; 8 * 1024];
        let manager = SessionManager::default();
        let open = manager
            .prepare("partial-export.log".into(), bytes.len() as u64)
            .unwrap();
        let destination = snapshot_path("partial.log");
        let mut exported = None;

        manager.index(
            &open.session_id,
            bytes.len() as u64,
            Cursor::new(bytes.clone()),
            |event| {
                if !event.done && exported.is_none() {
                    exported = Some(
                        manager
                            .export_snapshot(&open.session_id, &destination)
                            .unwrap(),
                    );
                }
            },
        );

        let result = exported.unwrap();
        assert_eq!(result.bytes, 4096);
        assert!(!result.complete);
        assert_eq!(std::fs::read(&destination).unwrap(), bytes[..4096]);
        std::fs::remove_file(destination).unwrap();
    }

    #[test]
    fn snapshot_export_rejects_missing_sessions_and_directory_targets() {
        let manager = SessionManager::default();
        let destination = snapshot_path("missing.log");
        assert!(manager.export_snapshot("missing", &destination).is_err());

        let open = manager.prepare("directory-target.log".into(), 0).unwrap();
        let directory = snapshot_path("directory");
        std::fs::create_dir(&directory).unwrap();
        let error = manager
            .export_snapshot(&open.session_id, &directory)
            .unwrap_err();
        assert!(error.to_string().contains("is a directory"));
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn closing_session_cancels_indexing_and_removes_the_cache() {
        let manager = Arc::new(SessionManager::default());
        let open = manager.prepare("cancel.log".into(), 128 * 1024).unwrap();
        let cache_path = manager
            .sessions
            .lock()
            .unwrap()
            .get(&open.session_id)
            .unwrap()
            .lock()
            .unwrap()
            .cache_path
            .clone();
        let closer = manager.clone();
        let closing_id = open.session_id.clone();
        let mut events = Vec::new();
        manager.index(
            &open.session_id,
            128 * 1024,
            Cursor::new(vec![b'x'; 128 * 1024]),
            |event| {
                events.push(event);
                closer.close(&closing_id);
            },
        );

        assert_eq!(events.len(), 1);
        assert!(!events[0].done);
        assert!(!cache_path.exists());
    }

    #[test]
    fn actual_bytes_over_the_limit_emit_a_terminal_failure() {
        let manager = SessionManager::default();
        let open = manager.prepare("limit.log".into(), 3).unwrap();
        let mut events = Vec::new();
        manager.index_with_limit(
            &open.session_id,
            3,
            Cursor::new(b"actual content"),
            3,
            |event| events.push(event),
        );

        assert_eq!(events.len(), 1);
        assert!(events[0].done);
        assert!(events[0].failed);
        assert!(events[0].error.as_deref().unwrap().contains("size limit"));
    }

    #[test]
    fn detects_and_reads_utf8_bom_and_gb18030() {
        let (utf8_manager, utf8_id, utf8_events) =
            indexed_manager(b"\xEF\xBB\xBFfirst\r\nsecond\n".to_vec());
        assert_eq!(utf8_events.last().unwrap().effective_encoding, "UTF-8");
        let utf8_lines = utf8_manager.read_lines(&utf8_id, 0, 10).unwrap();
        assert_eq!(utf8_lines[0].content, "first");
        assert_eq!(utf8_lines[1].content, "second");

        let (encoded, _, had_errors) = encoding_rs::GB18030.encode("中文😀\n第二行");
        assert!(!had_errors);
        let (gb_manager, gb_id, gb_events) = indexed_manager(encoded.into_owned());
        assert_eq!(gb_events.last().unwrap().effective_encoding, "GB18030");
        let gb_lines = gb_manager.read_lines(&gb_id, 0, 10).unwrap();
        assert_eq!(gb_lines[0].content, "中文😀");
        assert_eq!(gb_lines[1].content, "第二行");
    }

    #[test]
    fn indexes_utf16_in_both_byte_orders_and_strips_bom() {
        for (big_endian, expected_name) in [(false, "UTF-16LE"), (true, "UTF-16BE")] {
            let bytes = utf16_bytes("第一行\r\nsecond\n末行", big_endian, true);
            let (manager, session_id, events) = indexed_manager(bytes);
            assert_eq!(events.last().unwrap().effective_encoding, expected_name);
            let lines = manager.read_lines(&session_id, 0, 10).unwrap();
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[0].content, "第一行");
            assert_eq!(lines[1].content, "second");
            assert_eq!(lines[2].content, "末行");
        }
    }

    #[test]
    fn manual_encoding_change_rebuilds_offsets_and_latest_generation_wins() {
        let bytes = utf16_bytes("alpha\nbeta\ngamma", false, false);
        let (manager, session_id, _) = indexed_manager(bytes);
        let stale = manager
            .prepare_encoding_change(&session_id, "GB18030")
            .unwrap();
        let current = manager
            .prepare_encoding_change(&session_id, "UTF-16LE")
            .unwrap();
        let mut stale_events = Vec::new();
        manager.apply_encoding_change(stale, |event| stale_events.push(event));
        assert!(stale_events.is_empty());

        let mut events = Vec::new();
        manager.apply_encoding_change(current, |event| events.push(event));
        assert!(events.last().unwrap().done);
        assert_eq!(events.last().unwrap().encoding, "UTF-16LE");
        let lines = manager.read_lines(&session_id, 0, 10).unwrap();
        assert_eq!(
            lines
                .iter()
                .map(|line| line.content.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn line_index_handles_empty_lines_crlf_and_tail_without_newline() {
        let (manager, session_id, _) = indexed_manager(b"\nalpha\r\n\nomega".to_vec());
        assert_eq!(manager.line_count(&session_id), 4);
        let lines = manager.read_lines(&session_id, 0, 10).unwrap();
        assert_eq!(
            lines
                .iter()
                .map(|line| line.content.as_str())
                .collect::<Vec<_>>(),
            ["", "alpha", "", "omega"]
        );
        assert_eq!(
            manager.read_lines(&session_id, 3, 99).unwrap()[0].content,
            "omega"
        );
        assert!(manager.read_lines(&session_id, 4, 1).unwrap().is_empty());
        assert!(manager.read_lines(&session_id, 0, 0).unwrap().is_empty());
    }

    #[test]
    fn oversized_line_is_truncated_and_marked() {
        let mut bytes = vec![b'x'; MAX_LINE_BYTES + 100];
        bytes.push(b'\n');
        let (manager, session_id, _) = indexed_manager(bytes);
        let lines = manager.read_lines(&session_id, 0, 1).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].truncated);
        assert_eq!(lines[0].content.len(), MAX_LINE_BYTES);
    }

    #[test]
    fn close_and_lru_eviction_remove_cache_files() {
        let cache_dir = std::env::temp_dir().join(format!(
            "logcrate-index-test-{}-{}",
            std::process::id(),
            CACHE_TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let manager = SessionManager::default();
        manager.set_cache_dir(cache_dir.clone());

        let closed = manager.prepare("closed.log".into(), 0).unwrap();
        let closed_path = manager
            .sessions
            .lock()
            .unwrap()
            .get(&closed.session_id)
            .unwrap()
            .lock()
            .unwrap()
            .cache_path
            .clone();
        manager.close(&closed.session_id);
        assert!(!closed_path.exists());

        let first = manager.prepare("first.log".into(), 0).unwrap();
        let first_path = manager
            .sessions
            .lock()
            .unwrap()
            .get(&first.session_id)
            .unwrap()
            .lock()
            .unwrap()
            .cache_path
            .clone();
        let mut evicted = Vec::new();
        for index in 0..MAX_SESSIONS {
            evicted.extend(
                manager
                    .prepare(format!("extra-{index}.log"), 0)
                    .unwrap()
                    .evicted_session_ids,
            );
        }
        assert_eq!(evicted, vec![first.session_id.clone()]);
        assert_eq!(manager.line_count(&first.session_id), 0);
        assert!(!first_path.exists());
        manager.clear_all();
        let _ = std::fs::remove_dir(cache_dir);
    }
}
