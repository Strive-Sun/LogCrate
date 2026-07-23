use serde::Serialize;
use std::io;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchAccessStatus {
    Available,
    NeedsAuthorization,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileAccessErrorKind {
    PermissionDenied,
    NotFound,
    VolumeUnavailable,
    Io,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAccessError {
    pub kind: FileAccessErrorKind,
    pub message: String,
}

pub fn classify_io_error(error: &io::Error) -> FileAccessError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => FileAccessErrorKind::PermissionDenied,
        io::ErrorKind::NotFound => FileAccessErrorKind::NotFound,
        _ if matches!(error.raw_os_error(), Some(6 | 19)) => FileAccessErrorKind::VolumeUnavailable,
        _ => FileAccessErrorKind::Io,
    };
    FileAccessError {
        kind,
        message: error.to_string(),
    }
}

pub fn command_error(error: &io::Error) -> String {
    let classified = classify_io_error(error);
    let code = match classified.kind {
        FileAccessErrorKind::PermissionDenied => "FILE_ACCESS_DENIED",
        FileAccessErrorKind::NotFound => "FILE_NOT_FOUND",
        FileAccessErrorKind::VolumeUnavailable => "VOLUME_UNAVAILABLE",
        FileAccessErrorKind::Io => "FILE_IO_ERROR",
    };
    format!("{code}:{}", classified.message)
}

#[derive(Debug)]
pub struct RestoredBookmark {
    pub refreshed_bookmark: Option<String>,
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{command_error, RestoredBookmark, WatchAccessStatus};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{
        NSData, NSString, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSURL,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::sync::Mutex;

    struct AccessLease {
        url: Retained<NSURL>,
        started: bool,
        references: usize,
    }

    impl Drop for AccessLease {
        fn drop(&mut self) {
            if self.started {
                unsafe { self.url.stopAccessingSecurityScopedResource() };
            }
        }
    }

    #[derive(Default)]
    pub struct MacOsFileAccess {
        leases: Mutex<HashMap<String, AccessLease>>,
        failed: Mutex<HashSet<String>>,
    }

    impl MacOsFileAccess {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn create_bookmark(&self, path: &Path) -> Result<String, String> {
            let path = path
                .to_str()
                .ok_or_else(|| "BOOKMARK_INVALID_PATH:目录路径不是有效 Unicode".to_string())?;
            let path_string = NSString::from_str(path);
            let url = NSURL::fileURLWithPath_isDirectory(&path_string, true);
            let data = url
                .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                    NSURLBookmarkCreationOptions::WithSecurityScope,
                    None,
                    None,
                )
                .map_err(|error| format!("BOOKMARK_CREATE_FAILED:{error:?}"))?;
            Ok(STANDARD.encode(data.to_vec()))
        }

        pub fn restore_bookmark(
            &self,
            expected_path: &Path,
            encoded: &str,
        ) -> Result<RestoredBookmark, String> {
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| "BOOKMARK_INVALID_DATA:持久授权数据已损坏".to_string())?;
            let data = NSData::with_bytes(&bytes);
            let mut stale = Bool::NO;
            let url = unsafe {
                NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                    &data,
                    NSURLBookmarkResolutionOptions::WithSecurityScope,
                    None,
                    &mut stale,
                )
            }
            .map_err(|error| format!("BOOKMARK_RESOLVE_FAILED:{error:?}"))?;
            let resolved = url
                .path()
                .map(|path| path.to_string())
                .ok_or_else(|| "BOOKMARK_RESOLVE_FAILED:授权资源没有本地路径".to_string())?;
            let expected =
                std::fs::canonicalize(expected_path).unwrap_or_else(|_| expected_path.into());
            let actual_path = std::path::PathBuf::from(&resolved);
            let actual = std::fs::canonicalize(&actual_path).unwrap_or(actual_path);
            if expected != actual {
                return Err("BOOKMARK_IDENTITY_MISMATCH:授权资源与监控目录不一致".into());
            }

            let started = unsafe { url.startAccessingSecurityScopedResource() };
            if let Err(error) = std::fs::read_dir(&actual) {
                if started {
                    unsafe { url.stopAccessingSecurityScopedResource() };
                }
                return Err(command_error(&error));
            }

            let key = actual.to_string_lossy().into_owned();
            let mut leases = self.leases.lock().unwrap();
            if let Some(existing) = leases.get_mut(&key) {
                existing.references += 1;
                if started {
                    unsafe { url.stopAccessingSecurityScopedResource() };
                }
            } else {
                leases.insert(
                    key.clone(),
                    AccessLease {
                        url: url.clone(),
                        started,
                        references: 1,
                    },
                );
            }
            drop(leases);
            self.failed.lock().unwrap().remove(&key);

            let refreshed_bookmark = if stale.as_bool() {
                self.create_bookmark(&actual).ok()
            } else {
                None
            };
            Ok(RestoredBookmark { refreshed_bookmark })
        }

        pub fn release(&self, path: &Path) {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
            let key = canonical.to_string_lossy().into_owned();
            let mut leases = self.leases.lock().unwrap();
            if let Some(lease) = leases.get_mut(&key) {
                if lease.references > 1 {
                    lease.references -= 1;
                    return;
                }
            }
            leases.remove(&key);
            self.failed.lock().unwrap().remove(&key);
        }

        pub fn mark_needs_authorization(&self, path: &Path) {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
            self.failed
                .lock()
                .unwrap()
                .insert(canonical.to_string_lossy().into_owned());
        }

        pub fn status(&self, path: &Path, has_bookmark: bool) -> WatchAccessStatus {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
            let key = canonical.to_string_lossy().into_owned();
            if self.failed.lock().unwrap().contains(&key) {
                return WatchAccessStatus::NeedsAuthorization;
            }
            if self.leases.lock().unwrap().contains_key(&key) {
                return match std::fs::read_dir(&canonical) {
                    Ok(_) => WatchAccessStatus::Available,
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                        WatchAccessStatus::NeedsAuthorization
                    }
                    Err(_) => WatchAccessStatus::Unavailable,
                };
            }
            match std::fs::read_dir(&canonical) {
                Ok(_) if has_bookmark => WatchAccessStatus::Available,
                Ok(_) => WatchAccessStatus::NeedsAuthorization,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    WatchAccessStatus::NeedsAuthorization
                }
                Err(_) => WatchAccessStatus::Unavailable,
            }
        }
    }

    pub fn open_full_disk_access_settings() -> Result<bool, String> {
        let workspace = NSWorkspace::sharedWorkspace();
        for (index, target) in [
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
            "x-apple.systempreferences:com.apple.preference.security?Privacy",
        ]
        .iter()
        .enumerate()
        {
            let value = NSString::from_str(target);
            let Some(url) = NSURL::URLWithString(&value) else {
                continue;
            };
            if workspace.openURL(&url) {
                return Ok(index > 0);
            }
        }
        Err("SYSTEM_SETTINGS_OPEN_FAILED:无法打开 macOS 隐私与安全性设置".into())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{RestoredBookmark, WatchAccessStatus};
    use std::path::Path;

    #[derive(Default)]
    pub struct MacOsFileAccess;

    impl MacOsFileAccess {
        pub fn new() -> Self {
            Self
        }

        pub fn create_bookmark(&self, _path: &Path) -> Result<String, String> {
            Err("MACOS_ONLY:持久目录授权仅适用于 macOS".into())
        }

        pub fn restore_bookmark(
            &self,
            _expected_path: &Path,
            _encoded: &str,
        ) -> Result<RestoredBookmark, String> {
            Ok(RestoredBookmark {
                refreshed_bookmark: None,
            })
        }

        pub fn release(&self, _path: &Path) {}

        pub fn mark_needs_authorization(&self, _path: &Path) {}

        pub fn status(&self, path: &Path, _has_bookmark: bool) -> WatchAccessStatus {
            if path.is_dir() {
                WatchAccessStatus::Available
            } else {
                WatchAccessStatus::Unavailable
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::open_full_disk_access_settings;
pub use platform::MacOsFileAccess;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_io_errors() {
        assert_eq!(
            classify_io_error(&io::Error::from(io::ErrorKind::PermissionDenied)).kind,
            FileAccessErrorKind::PermissionDenied
        );
        assert_eq!(
            classify_io_error(&io::Error::from(io::ErrorKind::NotFound)).kind,
            FileAccessErrorKind::NotFound
        );
        assert_eq!(
            classify_io_error(&io::Error::from_raw_os_error(19)).kind,
            FileAccessErrorKind::VolumeUnavailable
        );
    }

    #[test]
    fn command_errors_have_stable_codes() {
        let error = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(command_error(&error).starts_with("FILE_ACCESS_DENIED:"));
    }
}
