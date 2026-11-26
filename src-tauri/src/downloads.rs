//! Downloads: every file a website downloads is listed here and in downloads.json.
//! Files go to the usual Windows Downloads folder.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::storage;

const FILE: &str = "downloads.json";
const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    InProgress,
    Done,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub path: String,
    pub file_name: String,
    pub state: State,
    /// Size in bytes, once finished.
    pub size: Option<u64>,
    pub time: i64,
}

#[derive(Default)]
pub struct Downloads(Mutex<Vec<Download>>);

pub fn load(app: &AppHandle) -> Downloads {
    let mut list: Vec<Download> = storage::read(app, FILE);
    // Downloads cut off when Raven closed did not finish.
    for download in &mut list {
        if download.state == State::InProgress {
            download.state = State::Failed;
        }
    }
    Downloads(Mutex::new(list))
}

fn changed(app: &AppHandle) {
    let list = app.state::<Downloads>().0.lock().unwrap().clone();
    if let Err(error) = storage::write(app, FILE, &list) {
        eprintln!("Could not save downloads: {error}");
    }
    let _ = app.emit("downloads", &list);
}

/// The name a file gets: the last part of its path, or of its address.
fn file_name_of(path: &Path, url: &str) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            url.split(['?', '#']).next()?.rsplit('/').find(|part| !part.is_empty()).map(String::from)
        })
        .unwrap_or_else(|| "indirme".into())
}

/// A free path in `folder`: "photo.png", else "photo (1).png", "photo (2).png"…
fn free_path(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..1000)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|candidate| !candidate.exists())
        .unwrap_or(first)
}

/// Called when a website starts a download. May change where the file goes.
pub fn started(app: &AppHandle, url: &str, destination: &mut PathBuf) {
    if destination.as_os_str().is_empty() || destination.is_relative() {
        if let Ok(folder) = app.path().download_dir() {
            let name = file_name_of(destination, url);
            *destination = free_path(&folder, &name);
        }
    }
    let download = Download {
        id: storage::now_ms(),
        url: url.to_string(),
        path: destination.to_string_lossy().to_string(),
        file_name: file_name_of(destination, url),
        state: State::InProgress,
        size: None,
        time: storage::now_ms(),
    };
    {
        let state = app.state::<Downloads>();
        let mut list = state.0.lock().unwrap();
        list.push(download);
        if list.len() > MAX_ENTRIES {
            let extra = list.len() - MAX_ENTRIES;
            list.drain(..extra);
        }
    }
    changed(app);
    let _ = app.emit("download-started", ());
}

/// Called when a download ends, well or not.
pub fn finished(app: &AppHandle, url: &str, path: Option<PathBuf>, success: bool) {
    {
        let state = app.state::<Downloads>();
        let mut list = state.0.lock().unwrap();
        let Some(download) = list
            .iter_mut()
            .rev()
            .find(|d| d.url == url && d.state == State::InProgress)
        else {
            return;
        };
        if let Some(path) = path.filter(|p| !p.as_os_str().is_empty()) {
            download.file_name = file_name_of(&path, url);
            download.path = path.to_string_lossy().to_string();
        }
        download.state = if success { State::Done } else { State::Failed };
        download.size = std::fs::metadata(&download.path).ok().map(|m| m.len());
    }
    changed(app);
}

fn path_of(app: &AppHandle, id: i64) -> Result<String, String> {
    app.state::<Downloads>()
        .0
        .lock()
        .unwrap()
        .iter()
        .find(|d| d.id == id)
        .map(|d| d.path.clone())
        .ok_or_else(|| "Download not found".into())
}

#[tauri::command]
pub async fn get_downloads(app: AppHandle) -> Vec<Download> {
    app.state::<Downloads>().0.lock().unwrap().clone()
}

#[tauri::command]
pub async fn open_download(app: AppHandle, id: i64) -> Result<(), String> {
    let path = path_of(&app, id)?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_download_in_folder(app: AppHandle, id: i64) -> Result<(), String> {
    let path = path_of(&app, id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// Removes a download from the list (the file stays on the computer).
#[tauri::command]
pub async fn remove_download(app: AppHandle, id: i64) {
    app.state::<Downloads>().0.lock().unwrap().retain(|d| d.id != id);
    changed(&app);
}

#[tauri::command]
pub async fn clear_downloads(app: AppHandle) {
    app.state::<Downloads>()
        .0
        .lock()
        .unwrap()
        .retain(|d| d.state == State::InProgress);
    changed(&app);
}

#[cfg(test)]
mod tests {
    use super::{file_name_of, free_path};
    use std::path::{Path, PathBuf};

    #[test]
    fn names_from_path_or_address() {
        assert_eq!(file_name_of(Path::new("C:/x/report.pdf"), ""), "report.pdf");
        assert_eq!(
            file_name_of(&PathBuf::new(), "https://example.com/files/setup.exe?v=2"),
            "setup.exe"
        );
        assert_eq!(file_name_of(&PathBuf::new(), "https://example.com/"), "example.com");
    }

    #[test]
    fn free_path_numbers_taken_names() {
        let folder = std::env::temp_dir().join(format!("raven-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.txt"), "x").unwrap();
        assert_eq!(free_path(&folder, "a.txt"), folder.join("a (1).txt"));
        assert_eq!(free_path(&folder, "b.txt"), folder.join("b.txt"));
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
