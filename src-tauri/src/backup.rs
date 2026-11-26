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

fn backup_json(app: &AppHandle, history_limit: usize) -> Result<String, String> {
    let backup = Backup {
        raven_backup: 1,
        bookmarks: app.state::<bookmarks::Bookmarks>().0.lock().unwrap().clone(),
        history: history::with_entries(app, |entries| {
            let start = entries.len().saturating_sub(history_limit);
            entries[start..].to_vec()
        }),
        settings: app.state::<settings::SettingsState>().0.lock().unwrap().clone(),
    };
    serde_json::to_string(&backup).map_err(|e| e.to_string())
}

/// The data sync sends (encrypted by the toolbar before it leaves).
#[tauri::command]
pub async fn get_sync_data(app: AppHandle) -> Result<String, String> {
    backup_json(&app, 3000)
}

#[tauri::command]
pub async fn export_backup(app: AppHandle) -> Result<String, String> {
    let text = backup_json(&app, usize::MAX)?;
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
    // This computer keeps its own sync connection.
    let own = app.state::<settings::SettingsState>().0.lock().unwrap().clone();
    let restored = settings::Settings {
        sync_url: own.sync_url,
        sync_key: own.sync_key,
        sync_secret: own.sync_secret,
        sync_id: own.sync_id,
        ..backup.settings
    };
    settings::apply_restored(&app, restored)?;
    Ok(Restored { bookmarks: added, history })
}
