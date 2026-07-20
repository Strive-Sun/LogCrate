use super::tar_reader::StreamCompression;
use super::{ArchiveEntry, ArchiveReader, EntryReader};
use std::path::{Path, PathBuf};

pub struct CompressedStreamReader {
    path: PathBuf,
    entry_name: String,
    compression: StreamCompression,
    compressed_size: u64,
}

impl CompressedStreamReader {
    pub fn open(path: &Path, compression: StreamCompression) -> anyhow::Result<Self> {
        let _ = compression.reader(path)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("log")
            .to_string();
        let entry_name = if compression == StreamCompression::Gzip {
            gzip_original_name(path).unwrap_or_else(|| remove_last_extension(&file_name))
        } else {
            remove_last_extension(&file_name)
        };
        Ok(Self {
            path: path.to_path_buf(),
            entry_name,
            compression,
            compressed_size: std::fs::metadata(path)?.len(),
        })
    }
}

fn remove_last_extension(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("log")
        .to_string()
}

fn gzip_original_name(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = flate2::read::GzDecoder::new(file);
    let name = decoder.header()?.filename()?;
    let name = std::str::from_utf8(name).ok()?;
    Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

impl ArchiveReader for CompressedStreamReader {
    fn entries(&mut self) -> anyhow::Result<Vec<ArchiveEntry>> {
        Ok(vec![ArchiveEntry::new(
            self.entry_name.clone(),
            self.compressed_size,
            false,
        )])
    }

    fn open_entry(&mut self, path: &str) -> anyhow::Result<EntryReader<'_>> {
        if path != self.entry_name {
            anyhow::bail!("条目不存在: {path}");
        }
        Ok(EntryReader::Sequential(
            self.compression.reader(&self.path)?,
        ))
    }
}
