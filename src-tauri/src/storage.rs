//! Small JSON files in Raven's data folder (session, bookmarks, history).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{de::DeserializeOwned, Serialize};
use tauri::{AppHandle, Manager};

fn path(app: &AppHandle, name: &str) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join(name))
}

/// Reads a file, or returns the default if it is missing or broken.
pub fn read<T: DeserializeOwned + Default>(app: &AppHandle, name: &str) -> T {
    path(app, name)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes a file safely: first to a temporary file, then renamed over the old one,
/// so a crash in the middle never leaves a half-written file.
pub fn write<T: Serialize>(app: &AppHandle, name: &str, value: &T) -> Result<(), String> {
    let path = path(app, name).ok_or("No data folder")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, text).map_err(|e| e.to_string())?;
    fs::rename(&temporary, &path).map_err(|e| e.to_string())
}

/// Milliseconds since 1970, used as a timestamp.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
