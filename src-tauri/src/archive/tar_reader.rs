use super::channel_reader::{copy_to_channel, send_error, ChannelReader};
use super::{ArchiveEntry, ArchiveReader, EntryReader};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::sync_channel;

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

pub struct TarArchiveReader {
    path: PathBuf,
    compression: StreamCompression,
}

impl TarArchiveReader {
    pub fn open(path: &Path, compression: StreamCompression) -> anyhow::Result<Self> {
        // Open the decoder now so corrupt headers fail at expansion time rather
        // than later in the indexing thread.
        let _ = compression.reader(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            compression,
        })
    }
}

fn safe_entry_path(path: &Path) -> Option<String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
    {
        return None;
    }
    let value = path.to_string_lossy().replace('\\', "/");
    super::is_safe_entry_name(&value).then_some(value)
}

impl ArchiveReader for TarArchiveReader {
    fn entries(&mut self) -> anyhow::Result<Vec<ArchiveEntry>> {
        let reader = self.compression.reader(&self.path)?;
        let mut archive = tar::Archive::new(reader);
        let mut result = Vec::new();
        for item in archive.entries()? {
            let item = item?;
            if !item.header().entry_type().is_file() {
                continue;
            }
            let Some(path) = safe_entry_path(&item.path()?) else {
                continue;
            };
            if result.len() >= super::MAX_ARCHIVE_ENTRIES {
                anyhow::bail!("归档条目数量超过安全上限");
            }
            result.push(ArchiveEntry::new(path, item.size(), false));
        }
        Ok(result)
    }

    fn open_entry(&mut self, path: &str) -> anyhow::Result<EntryReader<'_>> {
        let target = path.to_string();
        let source = self.path.clone();
        let compression = self.compression;
        let (sender, receiver) = sync_channel(2);
        std::thread::spawn(move || {
            let run = || -> anyhow::Result<()> {
                let reader = compression.reader(&source)?;
                let mut archive = tar::Archive::new(reader);
                for item in archive.entries()? {
                    let mut item = item?;
                    if !item.header().entry_type().is_file() {
                        continue;
                    }
                    if safe_entry_path(&item.path()?).as_deref() == Some(target.as_str()) {
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
