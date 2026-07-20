use super::channel_reader::{copy_to_channel, send_error, ChannelReader};
use super::{ArchiveEntry, ArchiveLimits, ArchiveReader, EntryReader};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::sync_channel;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCompression {
    None,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

impl StreamCompression {
    pub(crate) fn reader(self, path: &Path) -> anyhow::Result<Box<dyn Read + Send>> {
        let file = File::open(path)?;
        Ok(match self {
            Self::None => Box::new(file),
            Self::Gzip => Box::new(flate2::read::GzDecoder::new(file)),
            Self::Bzip2 => Box::new(bzip2::read::BzDecoder::new(file)),
            Self::Xz => Box::new(xz2::read::XzDecoder::new(file)),
            Self::Zstd => Box::new(zstd::stream::read::Decoder::new(file)?),
        })
    }
}

struct ScanLimitedReader {
    inner: Box<dyn Read + Send>,
    remaining: u64,
}

impl ScanLimitedReader {
    fn new(inner: Box<dyn Read + Send>, max_bytes: u64) -> Self {
        Self {
            inner,
            remaining: max_bytes,
        }
    }
}

impl Read for ScanLimitedReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "归档扫描解码内容超过字节安全上限",
            ));
        }
        let remaining = usize::try_from(self.remaining).unwrap_or(usize::MAX);
        let allowed = output.len().min(remaining);
        let count = self.inner.read(&mut output[..allowed])?;
        self.remaining = self.remaining.saturating_sub(count as u64);
        Ok(count)
    }
}

pub struct TarArchiveReader {
    path: PathBuf,
    compression: StreamCompression,
    limits: ArchiveLimits,
}

impl TarArchiveReader {
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn open(path: &Path, compression: StreamCompression) -> anyhow::Result<Self> {
        Self::open_with_limits(path, compression, ArchiveLimits::default())
    }

    pub fn open_with_limits(
        path: &Path,
        compression: StreamCompression,
        limits: ArchiveLimits,
    ) -> anyhow::Result<Self> {
        // Open the decoder now so corrupt headers fail at expansion time rather
        // than later in the indexing thread.
        let _ = compression.reader(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            compression,
            limits,
        })
    }
}

fn safe_entry_path(path: &Path, max_path_bytes: usize) -> Option<String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
    {
        return None;
    }
    let value = path.to_string_lossy().replace('\\', "/");
    super::is_safe_entry_name(&value, max_path_bytes).then_some(value)
}

impl ArchiveReader for TarArchiveReader {
    fn entries(&mut self) -> anyhow::Result<Vec<ArchiveEntry>> {
        let reader = ScanLimitedReader::new(
            self.compression.reader(&self.path)?,
            self.limits.max_scan_bytes,
        );
        let mut archive = tar::Archive::new(reader);
        let mut result = Vec::new();
        let started = Instant::now();
        for item in archive.entries()? {
            super::ensure_scan_time(started, self.limits)?;
            let item = item?;
            if !item.header().entry_type().is_file() {
                continue;
            }
            let Some(path) = safe_entry_path(&item.path()?, self.limits.max_path_bytes) else {
                continue;
            };
            if result.len() >= self.limits.max_entries {
                anyhow::bail!("归档条目数量超过安全上限");
            }
            result.push(ArchiveEntry::new(path, item.size(), false));
        }
        super::ensure_scan_time(started, self.limits)?;
        Ok(result)
    }

    fn open_entry(&mut self, path: &str) -> anyhow::Result<EntryReader<'_>> {
        let target = path.to_string();
        let source = self.path.clone();
        let compression = self.compression;
        let max_path_bytes = self.limits.max_path_bytes;
        let limits = self.limits;
        let (sender, receiver) = sync_channel(2);
        std::thread::spawn(move || {
            let run = || -> anyhow::Result<()> {
                let reader =
                    ScanLimitedReader::new(compression.reader(&source)?, limits.max_scan_bytes);
                let mut archive = tar::Archive::new(reader);
                let started = Instant::now();
                for item in archive.entries()? {
                    super::ensure_scan_time(started, limits)?;
                    let mut item = item?;
                    if !item.header().entry_type().is_file() {
                        continue;
                    }
                    if safe_entry_path(&item.path()?, max_path_bytes).as_deref()
                        == Some(target.as_str())
                    {
                        copy_to_channel(&mut item, &sender)?;
                        return Ok(());
                    }
                }
                anyhow::bail!("条目不存在: {target}")
            };
            if let Err(error) = run() {
                send_error(&sender, error);
            }
        });
        Ok(EntryReader::Sequential(Box::new(ChannelReader::new(
            receiver,
        ))))
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn directories_links_and_devices_are_not_exposed() {
        let path = std::env::temp_dir().join(format!(
            "logcrate-tar-special-{}-{}.tar",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let file = File::create(&path).unwrap();
        let mut builder = tar::Builder::new(file);

        let mut regular = tar::Header::new_gnu();
        regular.set_size(3);
        regular.set_mode(0o644);
        regular.set_cksum();
        builder
            .append_data(&mut regular, "safe.log", &b"ok\n"[..])
            .unwrap();

        let mut directory = tar::Header::new_gnu();
        directory.set_entry_type(tar::EntryType::Directory);
        directory.set_size(0);
        directory.set_mode(0o755);
        directory.set_cksum();
        builder
            .append_data(&mut directory, "folder/", io::empty())
            .unwrap();

        let mut symlink = tar::Header::new_gnu();
        symlink.set_entry_type(tar::EntryType::Symlink);
        symlink.set_size(0);
        symlink.set_mode(0o777);
        symlink.set_link_name("../outside.log").unwrap();
        symlink.set_cksum();
        builder
            .append_data(&mut symlink, "link.log", io::empty())
            .unwrap();

        let mut device = tar::Header::new_gnu();
        device.set_entry_type(tar::EntryType::Char);
        device.set_size(0);
        device.set_mode(0o600);
        device.set_cksum();
        builder
            .append_data(&mut device, "device.log", io::empty())
            .unwrap();
        builder.finish().unwrap();

        let mut archive = TarArchiveReader::open(&path, StreamCompression::None).unwrap();
        let entries = archive.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "safe.log");
        let _ = std::fs::remove_file(path);
    }
}
