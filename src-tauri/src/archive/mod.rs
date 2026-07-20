//! 归档读取抽象:屏蔽 zip / 裸文本差异(见技术设计 4.3)。
//! `entries()` 只读元信息(zip 仅读中央目录,不解压);
//! `open_entry()` 返回可流式读取的解压流。

mod channel_reader;
mod plain;
mod rar_reader;
mod sevenz_reader;
mod stream_reader;
mod tar_reader;
mod zip_reader;

use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub use plain::PlainReader;
pub use rar_reader::RarArchiveReader;
pub use sevenz_reader::SevenZipArchiveReader;
pub use stream_reader::CompressedStreamReader;
pub use tar_reader::{StreamCompression, TarArchiveReader};
pub use zip_reader::ZipArchiveReader;

static NESTED_CACHE_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    pub max_nested_depth: usize,
    pub max_decoded_bytes: u64,
    pub max_entries: usize,
    pub max_path_bytes: usize,
    pub max_scan_bytes: u64,
    pub max_scan_duration: Duration,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_nested_depth: 5,
            max_decoded_bytes: crate::index::MAX_UNCOMPRESSED,
            max_entries: 100_000,
            max_path_bytes: 4096,
            max_scan_bytes: 4 * 1024 * 1024 * 1024,
            max_scan_duration: Duration::from_secs(30),
        }
    }
}

/// Materialized ancestors for a lazily opened nested chain. Each ancestor is
/// streamed into the application cache only when its node is expanded. Files
/// are removed when the command/indexing thread releases this guard.
pub struct ResolvedArchiveChain {
    path: PathBuf,
    temporary_files: Vec<PathBuf>,
    decoded_bytes: u64,
}

impl ResolvedArchiveChain {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn decoded_bytes(&self) -> u64 {
        self.decoded_bytes
    }
}

impl Drop for ResolvedArchiveChain {
    fn drop(&mut self) {
        for path in self.temporary_files.iter().rev() {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn resolve_archive_chain(
    chain: &str,
    cache_dir: &Path,
) -> anyhow::Result<ResolvedArchiveChain> {
    resolve_archive_chain_with_limits(chain, cache_dir, ArchiveLimits::default())
}

pub fn resolve_archive_chain_with_limits(
    chain: &str,
    cache_dir: &Path,
    limits: ArchiveLimits,
) -> anyhow::Result<ResolvedArchiveChain> {
    let parts: Vec<&str> = chain.split("::").collect();
    if parts.is_empty() || parts[0].is_empty() {
        anyhow::bail!("归档路径为空");
    }
    if parts.len().saturating_sub(1) > limits.max_nested_depth {
        anyhow::bail!("嵌套归档超过最大深度 {}", limits.max_nested_depth);
    }
    std::fs::create_dir_all(cache_dir)?;
    let mut current = PathBuf::from(parts[0]);
    let mut temporary_files = Vec::new();
    let mut decoded_total = 0u64;

    for entry_path in &parts[1..] {
        let result = (|| -> anyhow::Result<PathBuf> {
            let mut reader = open_archive_with_limits(&current, limits)?;
            let entry = reader
                .entries()?
                .into_iter()
                .find(|entry| entry.path == *entry_path)
                .ok_or_else(|| anyhow::anyhow!("条目不存在: {entry_path}"))?;
            if entry.encrypted {
                anyhow::bail!("归档条目已加密，暂不支持密码输入: {entry_path}");
            }
            if !entry.is_archive {
                anyhow::bail!("条目不是受支持的归档: {entry_path}");
            }
            let suffix = Path::new(entry_path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("nested.archive")
                .replace(['/', '\\', ':'], "_");
            let output = cache_dir.join(format!(
                "nested-{}-{}-{suffix}",
                std::process::id(),
                NESTED_CACHE_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let mut source = reader.open_entry(entry_path)?;
            let mut destination = File::create(&output)?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                decoded_total = decoded_total.saturating_add(count as u64);
                if decoded_total > limits.max_decoded_bytes {
                    let _ = std::fs::remove_file(&output);
                    anyhow::bail!(
                        "嵌套归档累计解码内容超过 {} 字节安全上限",
                        limits.max_decoded_bytes
                    );
                }
                destination.write_all(&buffer[..count])?;
            }
            destination.flush()?;
            // Magic validation prevents a forged extension from creating a
            // misleading expandable node.
            if !detect_format(&output)?.is_archive() {
                let _ = std::fs::remove_file(&output);
                anyhow::bail!("嵌套条目内容不是受支持的归档: {entry_path}");
            }
            Ok(output)
        })();
        match result {
            Ok(output) => {
                current = output.clone();
                temporary_files.push(output);
            }
            Err(error) => {
                for path in temporary_files.iter().rev() {
                    let _ = std::fs::remove_file(path);
                }
                return Err(error);
            }
        }
    }

    Ok(ResolvedArchiveChain {
        path: current,
        temporary_files,
        decoded_bytes: decoded_total,
    })
}

pub(crate) fn is_safe_entry_name(name: &str, max_path_bytes: usize) -> bool {
    if name.is_empty()
        || name.len() > max_path_bytes
        || name.contains('\0')
        || name.starts_with(['/', '\\'])
        || name.contains("::")
    {
        return false;
    }
    let normalized = name.replace('\\', "/");
    !normalized
        .split('/')
        .any(|part| part == ".." || part.contains(':'))
}

pub(crate) fn ensure_scan_time(started: Instant, limits: ArchiveLimits) -> anyhow::Result<()> {
    if started.elapsed() > limits.max_scan_duration {
        anyhow::bail!("归档扫描超过 {:?} 时间上限", limits.max_scan_duration);
    }
    Ok(())
}

/// The single source of truth for archive detection across opening, directory
/// inventory, drag-and-drop and arrival notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    Rar,
    Tar,
    TarGzip,
    TarBzip2,
    TarXz,
    TarZstd,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Plain,
}

impl ArchiveFormat {
    #[allow(dead_code)]
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::SevenZip => "7z",
            Self::Rar => "RAR",
            Self::Tar => "TAR",
            Self::TarGzip => "tar.gz",
            Self::TarBzip2 => "tar.bz2",
            Self::TarXz => "tar.xz",
            Self::TarZstd => "tar.zst",
            Self::Gzip => "gzip",
            Self::Bzip2 => "bzip2",
            Self::Xz => "xz",
            Self::Zstd => "zstd",
            Self::Plain => "plain",
        }
    }

    pub fn is_archive(self) -> bool {
        !matches!(self, Self::Plain)
    }
}

/// 归档内的一个条目(仅元信息)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    /// 包内路径
    pub path: String,
    /// 解压后大小(中央目录声明值,仅供展示,不作安全上限依据)
    pub size: u64,
    /// 是否日志/文本
    pub is_log: bool,
    /// 是否加密条目(M1 不支持)
    pub encrypted: bool,
    /// Whether this entry is itself a supported archive and can be expanded lazily.
    pub is_archive: bool,
}

impl ArchiveEntry {
    pub(crate) fn new(path: String, size: u64, encrypted: bool) -> Self {
        let is_archive = is_archive_name(&path);
        Self {
            is_log: !encrypted && is_log_name(&path),
            path,
            size,
            encrypted,
            is_archive,
        }
    }
}

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// An opened entry explicitly exposes whether byte seeking is available.
pub enum EntryReader<'a> {
    Sequential(Box<dyn Read + 'a>),
    Seekable(Box<dyn ReadSeek + 'a>),
}

impl EntryReader<'_> {
    pub fn is_seekable(&self) -> bool {
        matches!(self, Self::Seekable(_))
    }

    pub fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::Seekable(reader) => reader.seek(position),
            Self::Sequential(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "archive entry is sequential",
            )),
        }
    }
}

impl Read for EntryReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Sequential(reader) => reader.read(buffer),
            Self::Seekable(reader) => reader.read(buffer),
        }
    }
}

/// 统一归档读取器
pub trait ArchiveReader: Send {
    /// 列出条目(不解压内容)
    fn entries(&mut self) -> anyhow::Result<Vec<ArchiveEntry>>;
    /// 打开某个条目,返回可流式读取的内容流
    fn open_entry(&mut self, path: &str) -> anyhow::Result<EntryReader<'_>>;
}

/// Construct a reader through the central format registry.
pub fn open_archive(path: &Path) -> anyhow::Result<Box<dyn ArchiveReader>> {
    open_archive_with_limits(path, ArchiveLimits::default())
}

pub fn open_archive_with_limits(
    path: &Path,
    limits: ArchiveLimits,
) -> anyhow::Result<Box<dyn ArchiveReader>> {
    let input_bytes = std::fs::metadata(path)?.len();
    if input_bytes > limits.max_scan_bytes {
        anyhow::bail!("归档扫描输入超过 {} 字节安全上限", limits.max_scan_bytes);
    }
    let started = Instant::now();
    let reader: Box<dyn ArchiveReader> = match detect_format(path)? {
        ArchiveFormat::Zip => Box::new(ZipArchiveReader::open_with_limits(path, limits)?),
        ArchiveFormat::SevenZip => Box::new(SevenZipArchiveReader::open_with_limits(path, limits)?),
        ArchiveFormat::Rar => Box::new(RarArchiveReader::open_with_limits(path, limits)?),
        ArchiveFormat::Tar => Box::new(TarArchiveReader::open_with_limits(
            path,
            StreamCompression::None,
            limits,
        )?),
        ArchiveFormat::TarGzip => Box::new(TarArchiveReader::open_with_limits(
            path,
            StreamCompression::Gzip,
            limits,
        )?),
        ArchiveFormat::TarBzip2 => Box::new(TarArchiveReader::open_with_limits(
            path,
            StreamCompression::Bzip2,
            limits,
        )?),
        ArchiveFormat::TarXz => Box::new(TarArchiveReader::open_with_limits(
            path,
            StreamCompression::Xz,
            limits,
        )?),
        ArchiveFormat::TarZstd => Box::new(TarArchiveReader::open_with_limits(
            path,
            StreamCompression::Zstd,
            limits,
        )?),
        ArchiveFormat::Gzip => {
            Box::new(CompressedStreamReader::open(path, StreamCompression::Gzip)?)
        }
        ArchiveFormat::Bzip2 => Box::new(CompressedStreamReader::open(
            path,
            StreamCompression::Bzip2,
        )?),
        ArchiveFormat::Xz => Box::new(CompressedStreamReader::open(path, StreamCompression::Xz)?),
        ArchiveFormat::Zstd => {
            Box::new(CompressedStreamReader::open(path, StreamCompression::Zstd)?)
        }
        ArchiveFormat::Plain => Box::new(PlainReader::open(path)?),
    };
    ensure_scan_time(started, limits)?;
    Ok(reader)
}

pub fn is_archive(path: &Path) -> anyhow::Result<bool> {
    Ok(detect_format(path)?.is_archive())
}

pub fn is_archive_name(name: &str) -> bool {
    format_from_name(name).is_some()
}

fn format_from_name(name: &str) -> Option<ArchiveFormat> {
    let lower = name.to_ascii_lowercase();
    let suffixes = [
        (".tar.bz2", ArchiveFormat::TarBzip2),
        (".tar.zst", ArchiveFormat::TarZstd),
        (".tar.gz", ArchiveFormat::TarGzip),
        (".tar.xz", ArchiveFormat::TarXz),
        (".tbz2", ArchiveFormat::TarBzip2),
        (".tzst", ArchiveFormat::TarZstd),
        (".tgz", ArchiveFormat::TarGzip),
        (".tbz", ArchiveFormat::TarBzip2),
        (".txz", ArchiveFormat::TarXz),
        (".zip", ArchiveFormat::Zip),
        (".7z", ArchiveFormat::SevenZip),
        (".rar", ArchiveFormat::Rar),
        (".tar", ArchiveFormat::Tar),
        (".gz", ArchiveFormat::Gzip),
        (".bz2", ArchiveFormat::Bzip2),
        (".xz", ArchiveFormat::Xz),
        (".zst", ArchiveFormat::Zstd),
    ];
    suffixes
        .iter()
        .find_map(|(suffix, format)| lower.ends_with(suffix).then_some(*format))
}

pub fn detect_format(path: &Path) -> anyhow::Result<ArchiveFormat> {
    let mut file = File::open(path)?;
    let mut head = [0u8; 512];
    let count = file.read(&mut head)?;
    let head = &head[..count];

    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        return Ok(ArchiveFormat::Zip);
    }
    if head.starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]) {
        return Ok(ArchiveFormat::SevenZip);
    }
    if head.starts_with(b"Rar!\x1a\x07\x00") || head.starts_with(b"Rar!\x1a\x07\x01\x00") {
        return Ok(ArchiveFormat::Rar);
    }
    if is_tar_header(head) {
        return Ok(ArchiveFormat::Tar);
    }

    let compression = if head.starts_with(&[0x1f, 0x8b]) {
        Some((
            StreamCompression::Gzip,
            ArchiveFormat::Gzip,
            ArchiveFormat::TarGzip,
        ))
    } else if head.starts_with(b"BZh") {
        Some((
            StreamCompression::Bzip2,
            ArchiveFormat::Bzip2,
            ArchiveFormat::TarBzip2,
        ))
    } else if head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        Some((
            StreamCompression::Xz,
            ArchiveFormat::Xz,
            ArchiveFormat::TarXz,
        ))
    } else if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Some((
            StreamCompression::Zstd,
            ArchiveFormat::Zstd,
            ArchiveFormat::TarZstd,
        ))
    } else {
        None
    };
    if let Some((compression, stream_format, tar_format)) = compression {
        let mut decoded = compression.reader(path)?;
        let mut tar_head = [0u8; 512];
        let mut filled = 0;
        while filled < tar_head.len() {
            let read = decoded.read(&mut tar_head[filled..])?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        let named_format = format_from_name(path.to_string_lossy().as_ref());
        let named_as_tar = named_format == Some(tar_format);
        let empty_tar = filled >= 512 && tar_head[..512].iter().all(|byte| *byte == 0);
        return Ok(
            if is_tar_header(&tar_head[..filled]) || (named_as_tar && empty_tar) {
                tar_format
            } else {
                stream_format
            },
        );
    }

    // An empty TAR is represented by zero blocks and has no regular header.
    // Other files must never be accepted solely because they were renamed.
    if matches!(
        format_from_name(path.to_string_lossy().as_ref()),
        Some(ArchiveFormat::Tar)
    ) && head.len() >= 512
        && head[..512].iter().all(|byte| *byte == 0)
    {
        return Ok(ArchiveFormat::Tar);
    }
    Ok(ArchiveFormat::Plain)
}

fn is_tar_header(head: &[u8]) -> bool {
    if head.len() < 512 {
        return false;
    }
    if &head[257..262] == b"ustar" {
        return true;
    }
    let stored = std::str::from_utf8(&head[148..156])
        .ok()
        .and_then(|value| u32::from_str_radix(value.trim_matches(['\0', ' ']), 8).ok());
    let Some(stored) = stored else {
        return false;
    };
    let calculated: u32 = head
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                b' ' as u32
            } else {
                *byte as u32
            }
        })
        .sum();
    stored == calculated
}

const LOG_EXTS: &[&str] = &["log", "txt", "out", "err", "trace", "json", "csv"];

/// 判定条目是否为日志/文本:扩展名优先,其次内容采样
pub fn is_log_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if let Some(ext) = Path::new(&lower).extension().and_then(|e| e.to_str()) {
        if LOG_EXTS.contains(&ext) {
            return true;
        }
        // 已知二进制扩展名直接判否
        const BIN_EXTS: &[&str] = &[
            "bin", "png", "jpg", "gz", "bz2", "xz", "zst", "zip", "7z", "rar", "tar", "exe", "dll",
            "so", "o",
        ];
        if BIN_EXTS.contains(&ext) {
            return false;
        }
    }
    false
}

/// 内容采样判定是否文本:检查前若干字节是否含 NUL 或过多不可打印字符
pub fn is_text_sample(sample: &[u8]) -> bool {
    if sample.is_empty() {
        return true;
    }
    if sample.contains(&0) {
        return false;
    }
    let non_print = sample
        .iter()
        .filter(|&&b| b < 0x09 || (b > 0x0d && b < 0x20))
        .count();
    (non_print as f64) / (sample.len() as f64) < 0.10
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQ: AtomicU64 = AtomicU64::new(1);

    struct FixtureDir(PathBuf);

    impl FixtureDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "logcrate-archive-formats-{}-{}",
                std::process::id(),
                FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tar_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            let content = b"hello from tar\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "logs/server.log", &content[..])
                .unwrap();
            let binary = [0, 1, 2, 3];
            let mut binary_header = tar::Header::new_gnu();
            binary_header.set_size(binary.len() as u64);
            binary_header.set_mode(0o644);
            binary_header.set_cksum();
            builder
                .append_data(&mut binary_header, "assets/image.bin", &binary[..])
                .unwrap();
            let unicode = "Unicode 日志\n".as_bytes();
            let mut unicode_header = tar::Header::new_gnu();
            unicode_header.set_size(unicode.len() as u64);
            unicode_header.set_mode(0o644);
            unicode_header.set_cksum();
            builder
                .append_data(&mut unicode_header, "日志/服务.txt", unicode)
                .unwrap();
            builder.finish().unwrap();
        }
        bytes
    }

    fn write_compressed(path: &Path, compression: StreamCompression, content: &[u8]) {
        let file = File::create(path).unwrap();
        match compression {
            StreamCompression::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(file, flate2::Compression::default());
                encoder.write_all(content).unwrap();
                encoder.finish().unwrap();
            }
            StreamCompression::Bzip2 => {
                let mut encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::default());
                encoder.write_all(content).unwrap();
                encoder.finish().unwrap();
            }
            StreamCompression::Xz => {
                let mut encoder = xz2::write::XzEncoder::new(file, 6);
                encoder.write_all(content).unwrap();
                encoder.finish().unwrap();
            }
            StreamCompression::Zstd => {
                let mut encoder = zstd::stream::write::Encoder::new(file, 3).unwrap();
                encoder.write_all(content).unwrap();
                encoder.finish().unwrap();
            }
            StreamCompression::None => std::fs::write(path, content).unwrap(),
        }
    }

    #[test]
    fn magic_wins_over_a_forged_extension() {
        let fixture = FixtureDir::new();
        let zip_named_text = fixture.path("fake.zip");
        std::fs::write(&zip_named_text, b"ordinary text").unwrap();
        assert_eq!(
            detect_format(&zip_named_text).unwrap(),
            ArchiveFormat::Plain
        );

        let extensionless_zip = fixture.path("download.part");
        std::fs::write(&extensionless_zip, b"PK\x05\x06\0\0\0\0").unwrap();
        assert_eq!(
            detect_format(&extensionless_zip).unwrap(),
            ArchiveFormat::Zip
        );
        assert!(is_archive_name("BUNDLE.TAR.GZ"));
        assert!(is_archive_name("diagnostics.TZST"));
        assert!(!is_archive_name("archive.zip.txt"));
        assert!(!is_safe_entry_name("../outside.log", 4096));
        assert!(!is_safe_entry_name("C:\\outside.log", 4096));
    }

    #[test]
    fn tar_and_compressed_tar_are_detected_and_streamed() {
        let fixture = FixtureDir::new();
        let tar = tar_bytes();
        let cases = [
            ("logs.tar", StreamCompression::None, ArchiveFormat::Tar),
            ("logs.tgz", StreamCompression::Gzip, ArchiveFormat::TarGzip),
            (
                "logs.tbz2",
                StreamCompression::Bzip2,
                ArchiveFormat::TarBzip2,
            ),
            ("logs.txz", StreamCompression::Xz, ArchiveFormat::TarXz),
            ("logs.tzst", StreamCompression::Zstd, ArchiveFormat::TarZstd),
        ];
        for (name, compression, expected) in cases {
            let path = fixture.path(name);
            write_compressed(&path, compression, &tar);
            assert_eq!(detect_format(&path).unwrap(), expected);
            let mut archive = open_archive(&path).unwrap();
            let entries = archive.entries().unwrap();
            assert_eq!(entries.len(), 3);
            assert!(entries
                .iter()
                .any(|entry| entry.path == "logs/server.log" && entry.is_log));
            assert!(entries
                .iter()
                .any(|entry| entry.path == "assets/image.bin" && !entry.is_log));
            assert!(entries
                .iter()
                .any(|entry| entry.path == "日志/服务.txt" && entry.is_log));
            let mut content = String::new();
            archive
                .open_entry("logs/server.log")
                .unwrap()
                .read_to_string(&mut content)
                .unwrap();
            assert_eq!(content, "hello from tar\n");
        }
    }

    #[test]
    fn single_file_streams_synthesize_a_readable_entry() {
        let fixture = FixtureDir::new();
        let cases = [
            ("server.log.gz", StreamCompression::Gzip),
            ("server.log.bz2", StreamCompression::Bzip2),
            ("server.log.xz", StreamCompression::Xz),
            ("server.log.zst", StreamCompression::Zstd),
        ];
        for (name, compression) in cases {
            let path = fixture.path(name);
            write_compressed(&path, compression, b"line one\nline two\n");
            let mut archive = open_archive(&path).unwrap();
            let entries = archive.entries().unwrap();
            assert_eq!(entries[0].path, "server.log");
            assert!(entries[0].is_log);
            let mut content = String::new();
            archive
                .open_entry("server.log")
                .unwrap()
                .read_to_string(&mut content)
                .unwrap();
            assert_eq!(content, "line one\nline two\n");
        }
    }

    #[test]
    fn empty_tar_combinations_and_truncated_streams_are_handled() {
        let fixture = FixtureDir::new();
        let empty_tar = vec![0u8; 1024];
        let cases = [
            ("empty.tar", StreamCompression::None, ArchiveFormat::Tar),
            ("empty.tgz", StreamCompression::Gzip, ArchiveFormat::TarGzip),
            (
                "empty.tbz2",
                StreamCompression::Bzip2,
                ArchiveFormat::TarBzip2,
            ),
            ("empty.txz", StreamCompression::Xz, ArchiveFormat::TarXz),
            (
                "empty.tzst",
                StreamCompression::Zstd,
                ArchiveFormat::TarZstd,
            ),
        ];
        for (name, compression, expected) in cases {
            let path = fixture.path(name);
            write_compressed(&path, compression, &empty_tar);
            assert_eq!(detect_format(&path).unwrap(), expected);
            assert!(open_archive(&path).unwrap().entries().unwrap().is_empty());
        }

        for (name, bytes) in [
            ("broken.gz", &[0x1f, 0x8b, 0x08][..]),
            ("broken.bz2", b"BZh"),
            ("broken.xz", &[0xfd, b'7', b'z', b'X', b'Z', 0x00][..]),
            ("broken.zst", &[0x28, 0xb5, 0x2f, 0xfd][..]),
        ] {
            let path = fixture.path(name);
            std::fs::write(&path, bytes).unwrap();
            assert!(detect_format(&path).is_err(), "{name} was accepted");
        }
    }

    #[test]
    fn nested_archive_is_materialized_only_when_the_chain_is_resolved() {
        let fixture = FixtureDir::new();
        let inner = tar_bytes();
        let outer_path = fixture.path("outer.zip");
        {
            let file = File::create(&outer_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file(
                "nested/diagnostics.tar",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(&inner).unwrap();
            zip.finish().unwrap();
        }
        let cache = fixture.path("cache");
        assert!(!cache.exists());
        let chain = format!("{}::nested/diagnostics.tar", outer_path.to_string_lossy());
        let resolved = resolve_archive_chain(&chain, &cache).unwrap();
        assert!(resolved.path().exists());
        let nested_path = resolved.path().to_path_buf();
        let mut archive = open_archive(resolved.path()).unwrap();
        assert_eq!(archive.entries().unwrap()[0].path, "logs/server.log");
        drop(archive);
        drop(resolved);
        assert!(!nested_path.exists());
    }

    #[test]
    fn nested_chain_rejects_more_than_five_layers_before_reading() {
        let fixture = FixtureDir::new();
        let chain = "missing.zip::1.zip::2.zip::3.zip::4.zip::5.zip::6.zip";
        let error = match resolve_archive_chain(chain, &fixture.path("cache")) {
            Ok(_) => panic!("over-deep chain unexpectedly resolved"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("最大深度 5"));
    }

    #[test]
    fn injectable_limits_reject_entry_count_paths_and_nested_bytes() {
        let fixture = FixtureDir::new();
        let tar_path = fixture.path("limited.tar");
        std::fs::write(&tar_path, tar_bytes()).unwrap();

        let entry_limit = ArchiveLimits {
            max_entries: 2,
            ..ArchiveLimits::default()
        };
        let mut archive = open_archive_with_limits(&tar_path, entry_limit).unwrap();
        assert!(archive
            .entries()
            .unwrap_err()
            .to_string()
            .contains("条目数量"));

        let path_limit = ArchiveLimits {
            max_path_bytes: 8,
            ..ArchiveLimits::default()
        };
        let mut archive = open_archive_with_limits(&tar_path, path_limit).unwrap();
        assert!(archive.entries().unwrap().is_empty());

        let input_limit = ArchiveLimits {
            max_scan_bytes: 16,
            ..ArchiveLimits::default()
        };
        let error = match open_archive_with_limits(&tar_path, input_limit) {
            Ok(_) => panic!("oversized scan input unexpectedly opened"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("扫描输入"));

        let compressed_tar = fixture.path("scan-limit.tar.gz");
        write_compressed(&compressed_tar, StreamCompression::Gzip, &tar_bytes());
        let compressed_bytes = std::fs::metadata(&compressed_tar).unwrap().len();
        let decoded_scan_limit = ArchiveLimits {
            max_scan_bytes: compressed_bytes + 512,
            ..ArchiveLimits::default()
        };
        let mut archive = open_archive_with_limits(&compressed_tar, decoded_scan_limit).unwrap();
        assert!(archive
            .entries()
            .unwrap_err()
            .to_string()
            .contains("扫描解码内容"));

        let duration_limit = ArchiveLimits {
            max_scan_duration: Duration::ZERO,
            ..ArchiveLimits::default()
        };
        let error = match open_archive_with_limits(&tar_path, duration_limit) {
            Ok(_) => panic!("zero-duration scan unexpectedly opened"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("时间上限"));

        let outer_path = fixture.path("outer-limit.zip");
        {
            let file = File::create(&outer_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("inner.tar", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&tar_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let cache = fixture.path("limited-cache");
        let chain = format!("{}::inner.tar", outer_path.to_string_lossy());
        let byte_limit = ArchiveLimits {
            max_decoded_bytes: 128,
            ..ArchiveLimits::default()
        };
        let error = match resolve_archive_chain_with_limits(&chain, &cache, byte_limit) {
            Ok(_) => panic!("oversized nested archive unexpectedly resolved"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("128 字节"));
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }

    #[test]
    fn same_format_nesting_and_forged_nested_suffix_are_lazy_and_safe() {
        let fixture = FixtureDir::new();
        let inner_path = fixture.path("inner.zip");
        {
            let file = File::create(&inner_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("inside.log", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"nested zip log\n").unwrap();
            zip.finish().unwrap();
        }
        let outer_path = fixture.path("outer.zip");
        {
            let file = File::create(&outer_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("inner.zip", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&std::fs::read(&inner_path).unwrap()).unwrap();
            zip.start_file("fake.tar", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"not an archive").unwrap();
            zip.finish().unwrap();
        }

        let cache = fixture.path("same-format-cache");
        let nested = format!("{}::inner.zip", outer_path.to_string_lossy());
        let resolved = resolve_archive_chain(&nested, &cache).unwrap();
        let mut reader = open_archive(resolved.path()).unwrap();
        assert_eq!(reader.entries().unwrap()[0].path, "inside.log");
        drop(reader);
        drop(resolved);
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);

        let forged = format!("{}::fake.tar", outer_path.to_string_lossy());
        let error = match resolve_archive_chain(&forged, &cache) {
            Ok(_) => panic!("forged nested suffix unexpectedly resolved"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("不是受支持的归档"));
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);

        let replacement = fixture.path("replacement.zip");
        {
            let file = File::create(&replacement).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("changed.log", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"changed parent\n").unwrap();
            zip.finish().unwrap();
        }
        {
            let file = File::create(&outer_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("inner.zip", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&std::fs::read(&replacement).unwrap())
                .unwrap();
            zip.finish().unwrap();
        }
        let resolved = resolve_archive_chain(&nested, &cache).unwrap();
        let mut reader = open_archive(resolved.path()).unwrap();
        assert_eq!(reader.entries().unwrap()[0].path, "changed.log");
        drop(reader);
        drop(resolved);
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }
}
