//! Backup: bookmarks, history and settings in one JSON file (in Downloads),
//! and restoring it on another computer. A simple stand-in until sync.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{bookmarks, history, settings};

#[derive(Deserialize, Serialize)]
struct Backup {
    raven_backup: u32,
    bookmarks: Vec<bookmarks::Bookmark>,
    history: Vec<history::Entry>,
    settings: settings::Settings,
}

#[tauri::command]
pub async fn export_backup(app: AppHandle) -> Result<String, String> {
    let backup = Backup {
        raven_backup: 1,
        bookmarks: app.state::<bookmarks::Bookmarks>().0.lock().unwrap().clone(),
        history: history::with_entries(&app, |entries| entries.to_vec()),
        settings: app.state::<settings::SettingsState>().0.lock().unwrap().clone(),
    };
    let text = serde_json::to_string(&backup).map_err(|e| e.to_string())?;
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let name = format!("Raven yedeği {}.json", crate::storage::now_ms());
    std::fs::write(dir.join(&name), text).map_err(|e| e.to_string())?;
    Ok(name)
}

#[derive(Serialize)]
pub struct Restored {
    bookmarks: usize,
    history: usize,
}

/// Adds a backup's bookmarks and history to this Raven, and takes its settings.
#[tauri::command]
pub async fn import_backup(app: AppHandle, text: String) -> Result<Restored, String> {
    let backup: Backup = serde_json::from_str(&text).map_err(|_| "Bu bir Raven yedeği değil")?;
    let added = bookmarks::add_many(&app, backup.bookmarks.into_iter().map(|b| (b.title, b.url)).collect());
    let history = history::import(&app, backup.history.into_iter().map(|e| (e.url, e.title, e.time, 1)).collect());
    settings::apply_restored(&app, backup.settings)?;
    Ok(Restored { bookmarks: added, history })
}
