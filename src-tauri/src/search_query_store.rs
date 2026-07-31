use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const FS_RETRIES: usize = 20;
const FS_RETRY_DELAY: Duration = Duration::from_millis(50);

pub(crate) fn staging_path(active: &Path) -> PathBuf {
    let mut value = active.as_os_str().to_os_string();
    value.push(".next");
    PathBuf::from(value)
}

pub(crate) fn previous_path(active: &Path) -> PathBuf {
    let mut value = active.as_os_str().to_os_string();
    value.push(".previous");
    PathBuf::from(value)
}

pub(crate) fn recover_directories(active: &Path) -> anyhow::Result<()> {
    let staging = staging_path(active);
    let previous = previous_path(active);
    if !active.exists() {
        if previous.exists() {
            retry_fs("recover-previous", &previous, Some(active), || {
                fs::rename(&previous, active)
            })?;
        } else if staging.exists() {
            retry_fs("recover-staging", &staging, Some(active), || {
                fs::rename(&staging, active)
            })?;
        }
    }
    if active.exists() && previous.exists() {
        retry_fs("remove-recovered-previous", &previous, None, || {
            fs::remove_dir_all(&previous)
        })?;
    }
    if active.exists() && staging.exists() {
        retry_fs("remove-interrupted-staging", &staging, None, || {
            fs::remove_dir_all(&staging)
        })?;
    }
    Ok(())
}

pub(crate) fn retry_fs<T, F>(
    stage: &str,
    source: &Path,
    destination: Option<&Path>,
    mut operation: F,
) -> anyhow::Result<T>
where
    F: FnMut() -> std::io::Result<T>,
{
    let mut attempts = 0;
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let retryable = cfg!(windows)
                    && matches!(error.raw_os_error(), Some(5 | 32 | 33))
                    && attempts < FS_RETRIES;
                if retryable {
                    attempts += 1;
                    std::thread::sleep(FS_RETRY_DELAY);
                    continue;
                }
                let target = destination
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<none>".into());
                return Err(anyhow::anyhow!(
                    "query-index stage={stage} source={} target={target} attempts={}: {error}",
                    source.display(),
                    attempts + 1
                ));
            }
        }
    }
}
