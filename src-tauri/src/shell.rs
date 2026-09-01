#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(not(target_os = "windows"))]
use std::process::Command;
#[cfg(target_os = "windows")]
use std::ptr::{null, null_mut};

use url::Url;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Shell::ShellExecuteW;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::errors::{AppError, AppResult};

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
pub fn open_target(target: &str, context: &'static str) -> AppResult<()> {
    let operation = wide_null("open");
    let target = wide_null(target);
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    if result > 32 {
        Ok(())
    } else {
        Err(AppError::msg(format!(
            "{context}: ShellExecute code {result}"
        )))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn open_target(target: &str, context: &'static str) -> AppResult<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    Command::new(opener)
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|source| AppError::Io { context, source })
}

pub fn open_external_url(raw_url: &str) -> AppResult<()> {
    let parsed = Url::parse(raw_url).map_err(|source| AppError::Url {
        context: "Failed to parse external URL",
        source,
    })?;

    match parsed.scheme() {
        "http" | "https" => open_target(parsed.as_str(), "Failed to open browser"),
        _ => Err(AppError::msg("Only http and https URLs can be opened")),
    }
}
