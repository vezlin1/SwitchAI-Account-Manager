use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration;

use crate::errors::{AppError, AppResult};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

const REPLACE_ATTEMPTS: usize = 5;
#[cfg(target_os = "windows")]
const REPLACE_RETRY_DELAY: Duration = Duration::from_millis(50);

fn sibling_path(path: &Path, suffix: &str) -> AppResult<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::msg("Atomic file target has no valid file name"))?;
    Ok(path.with_file_name(format!("{file_name}{suffix}")))
}

pub fn backup_path(path: &Path) -> AppResult<PathBuf> {
    sibling_path(path, ".bak")
}

fn temporary_path(path: &Path) -> AppResult<PathBuf> {
    let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    sibling_path(path, &format!(".tmp-{}-{sequence}", std::process::id()))
}

#[cfg(target_os = "windows")]
fn retryable_replace_error(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

#[cfg(not(target_os = "windows"))]
fn retryable_replace_error(_error: &io::Error) -> bool {
    false
}

fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    let mut last_error = None;

    for attempt in 0..REPLACE_ATTEMPTS {
        match fs::rename(temp_path, target_path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let should_retry =
                    attempt + 1 < REPLACE_ATTEMPTS && retryable_replace_error(&error);
                last_error = Some(error);
                if !should_retry {
                    break;
                }

                #[cfg(target_os = "windows")]
                thread::sleep(REPLACE_RETRY_DELAY);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| io::Error::other("Atomic replace failed")))
}

#[cfg(unix)]
fn restrict_to_current_user(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_to_current_user(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(file) = File::open(parent) {
        let _ = file.sync_all();
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::msg("Atomic file target has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|source| AppError::Io {
        context: "Failed to create atomic file directory",
        source,
    })?;

    let temp_path = temporary_path(path)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|source| AppError::Io {
                context: "Failed to create temporary file",
                source,
            })?;

        if private {
            restrict_to_current_user(&file).map_err(|source| AppError::Io {
                context: "Failed to restrict temporary file permissions",
                source,
            })?;
        }

        file.write_all(bytes).map_err(|source| AppError::Io {
            context: "Failed to write temporary file",
            source,
        })?;
        file.sync_all().map_err(|source| AppError::Io {
            context: "Failed to sync temporary file",
            source,
        })?;
        drop(file);

        replace_file(&temp_path, path).map_err(|source| AppError::Io {
            context: "Failed to atomically replace file",
            source,
        })?;
        sync_parent_directory(path).map_err(|source| AppError::Io {
            context: "Failed to sync atomic file directory",
            source,
        })?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub fn write_atomic_with_backup(path: &Path, bytes: &[u8], private: bool) -> AppResult<()> {
    if path.exists() {
        let backup = backup_path(path)?;
        fs::copy(path, &backup).map_err(|source| AppError::Io {
            context: "Failed to copy file before backup",
            source,
        })?;
    }

    write_atomic(path, bytes, private)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::{backup_path, write_atomic, write_atomic_with_backup};

    fn test_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("vg-atomic-file-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    #[test]
    fn writes_and_replaces_file_without_temp_artifacts() {
        let dir = test_dir();
        let path = dir.join("state.json");

        write_atomic(&path, b"first", false).expect("first atomic write");
        write_atomic(&path, b"second", false).expect("second atomic write");

        assert_eq!(fs::read(&path).expect("read result"), b"second");
        assert_eq!(fs::read_dir(&dir).expect("list directory").count(), 1);
        fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn keeps_previous_contents_in_backup() {
        let dir = test_dir();
        let path = dir.join("state.json");

        write_atomic_with_backup(&path, b"first", false).expect("first write");
        write_atomic_with_backup(&path, b"second", false).expect("second write");

        assert_eq!(fs::read(&path).expect("read result"), b"second");
        assert_eq!(
            fs::read(backup_path(&path).expect("backup path")).expect("read backup"),
            b"first"
        );
        fs::remove_dir_all(dir).expect("remove test directory");
    }
}
