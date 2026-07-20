use super::channel_reader::{copy_to_channel, send_error, ChannelReader};
use super::{ArchiveEntry, ArchiveReader, EntryReader};
use sevenz_rust::{Password, SevenZReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::sync_channel;

pub struct SevenZipArchiveReader {
    path: PathBuf,
}

impl SevenZipArchiveReader {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        SevenZReader::open(path, Password::empty()).map_err(map_error)?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

fn map_error(error: sevenz_rust::Error) -> anyhow::Error {
    match error {
        sevenz_rust::Error::PasswordRequired | sevenz_rust::Error::MaybeBadPassword(_) => {
            anyhow::anyhow!("归档已加密，暂不支持密码输入")
        }
        other => anyhow::anyhow!("7z 归档读取失败: {other}"),
    }
}

fn is_regular_entry(entry: &sevenz_rust::SevenZArchiveEntry) -> bool {
    if entry.is_directory() || entry.is_anti_item() {
        return false;
    }
    if !entry.has_windows_attributes {
        return true;
    }
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    if entry.windows_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return false;
    }
    let unix_mode = entry.windows_attributes >> 16;
    let kind = unix_mode & 0o170000;
    kind == 0 || kind == 0o100000
}

impl ArchiveReader for SevenZipArchiveReader {
    fn entries(&mut self) -> anyhow::Result<Vec<ArchiveEntry>> {
        let archive = SevenZReader::open(&self.path, Password::empty()).map_err(map_error)?;
        let entries = archive
            .archive()
            .files
            .iter()
            .filter(|entry| is_regular_entry(entry) && super::is_safe_entry_name(&entry.name))
            .map(|entry| ArchiveEntry::new(entry.name.clone(), entry.size, false))
            .collect::<Vec<_>>();
        if entries.len() > super::MAX_ARCHIVE_ENTRIES {
            anyhow::bail!("归档条目数量超过安全上限");
        }
        Ok(entries)
    }

    fn open_entry(&mut self, path: &str) -> anyhow::Result<EntryReader<'_>> {
        let source = self.path.clone();
        let target = path.to_string();
        let (sender, receiver) = sync_channel(2);
        std::thread::spawn(move || {
            let run = || -> anyhow::Result<()> {
                let mut archive =
                    SevenZReader::open(&source, Password::empty()).map_err(map_error)?;
                let mut found = false;
                archive
                    .for_each_entries(|entry, reader| {
                        if entry.name == target {
                            found = true;
                            copy_to_channel(reader, &sender).map_err(sevenz_rust::Error::from)?;
                            return Ok(false);
                        }
                        Ok(true)
                    })
                    .map_err(map_error)?;
                if !found {
                    anyhow::bail!("条目不存在: {target}");
                }
                Ok(())
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
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn lzma_entry_is_listed_and_streamed() {
        let dir = std::env::temp_dir().join(format!("logcrate-7z-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.7z");
        std::fs::write(
            &path,
            [
                0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00, 0x03, 0x17, 0xcf, 0x0e, 0x01, 0x18, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x68, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0xd2, 0x89, 0x89, 0x22, 0x00, 0x3a, 0x1a, 0x09, 0x67, 0x7e, 0xae, 0x72, 0xdc, 0x7c,
                0x84, 0xc1, 0xca, 0xe3, 0x8a, 0xb0, 0x69, 0xf4, 0x5c, 0xff, 0xfe, 0x74, 0x20, 0x00,
                0x01, 0x04, 0x06, 0x00, 0x01, 0x09, 0x18, 0x00, 0x07, 0x0b, 0x01, 0x00, 0x01, 0x23,
                0x03, 0x01, 0x01, 0x05, 0x5d, 0x00, 0x00, 0x80, 0x00, 0x0c, 0x0f, 0x00, 0x08, 0x0a,
                0x01, 0x15, 0xaf, 0x50, 0x66, 0x00, 0x00, 0x05, 0x01, 0x11, 0x13, 0x00, 0x66, 0x00,
                0x69, 0x00, 0x6c, 0x00, 0x65, 0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00,
                0x00, 0x00, 0x14, 0x0a, 0x01, 0x00, 0x48, 0x17, 0x2d, 0x99, 0x4f, 0xa7, 0xd7, 0x01,
                0x12, 0x0a, 0x01, 0x00, 0x48, 0x17, 0x2d, 0x99, 0x4f, 0xa7, 0xd7, 0x01, 0x13, 0x0a,
                0x01, 0x00, 0x48, 0x17, 0x2d, 0x99, 0x4f, 0xa7, 0xd7, 0x01, 0x15, 0x06, 0x01, 0x00,
                0x20, 0x80, 0xb4, 0x81, 0x00, 0x00,
            ],
        )
        .unwrap();
        let mut reader = SevenZipArchiveReader::open(&path).unwrap();
        let entries = reader.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "file.txt");
        let mut content = String::new();
        reader
            .open_entry("file.txt")
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "this is a file\n");
        let _ = std::fs::remove_dir_all(dir);
    }
}
