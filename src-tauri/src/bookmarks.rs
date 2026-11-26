//! Bookmarks, saved to bookmarks.json, and the bookmarks bar under the address bar.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Webview};

use crate::{settings::SettingsState, storage, tabs, windows};

const FILE: &str = "bookmarks.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bookmark {
    pub id: i64,
    pub title: String,
    pub url: String,
}

#[derive(Default)]
pub struct Bookmarks(pub Mutex<Vec<Bookmark>>);

pub fn load(app: &AppHandle) -> Bookmarks {
    Bookmarks(Mutex::new(storage::read(app, FILE)))
}

fn list(app: &AppHandle) -> Vec<Bookmark> {
    app.state::<Bookmarks>().0.lock().unwrap().clone()
}

/// Saves, tells the toolbar, and shows or hides the bookmarks bar.
fn changed(app: &AppHandle) {
    let bookmarks = list(app);
    if let Err(error) = storage::write(app, FILE, &bookmarks) {
        eprintln!("Could not save bookmarks: {error}");
    }
    let _ = app.emit("bookmarks", &bookmarks);
    refresh_bar(app);
}

/// Adds bookmarks that are not there yet (same address). Returns how many were added.
pub fn add_many(app: &AppHandle, items: Vec<(String, String)>) -> usize {
    let added = {
        let state = app.state::<Bookmarks>();
        let mut list = state.0.lock().unwrap();
        let mut next = list.iter().map(|b| b.id).max().unwrap_or(0);
        let mut added = 0;
        for (title, url) in items {
            if list.iter().any(|b| b.url == url) {
                continue;
            }
            next += 1;
            list.push(Bookmark { id: next, title, url });
            added += 1;
        }
        added
    };
    if added > 0 {
        changed(app);
    }
    added
}

/// The bar shows when it is switched on and there is at least one bookmark.
pub fn refresh_bar(app: &AppHandle) {
    let enabled = app.state::<SettingsState>().0.lock().unwrap().show_bookmarks_bar;
    let any = !app.state::<Bookmarks>().0.lock().unwrap().is_empty();
    windows::set_bookmarks_bar(app, enabled && any);
}

/// Adds a window's active page to the bookmarks, or removes it if it is already
/// there (the star in the address bar, Ctrl+D).
pub fn toggle_active(app: &AppHandle, window: &str) -> Result<bool, String> {
    let (url, title) = tabs::active_page(app, window).ok_or("No open tab")?;
    if url.is_empty() {
        return Err("The new tab page cannot be bookmarked".into());
    }
    let added = {
        let state = app.state::<Bookmarks>();
        let mut bookmarks = state.0.lock().unwrap();
        if let Some(index) = bookmarks.iter().position(|b| b.url == url) {
            bookmarks.remove(index);
            false
        } else {
            let title = if title.trim().is_empty() { url.clone() } else { title };
            bookmarks.push(Bookmark { id: storage::now_ms(), title, url });
            true
        }
    };
    changed(app);
    Ok(added)
}

/// Shows or hides the bookmarks bar (Ctrl+Shift+B).
pub fn toggle_bar(app: &AppHandle) -> Result<(), String> {
    let settings = {
        let state = app.state::<SettingsState>();
        let mut settings = state.0.lock().unwrap();
        settings.show_bookmarks_bar = !settings.show_bookmarks_bar;
        settings.clone()
    };
    crate::settings::store(app, &settings)?;
    refresh_bar(app);
    Ok(())
}

#[tauri::command]
pub async fn get_bookmarks(app: AppHandle) -> Vec<Bookmark> {
    list(&app)
}

#[tauri::command]
pub async fn toggle_bookmark(app: AppHandle, webview: Webview) -> Result<bool, String> {
    toggle_active(&app, &windows::window_of(&webview))
}

#[tauri::command]
pub async fn remove_bookmark(app: AppHandle, id: i64) {
    app.state::<Bookmarks>().0.lock().unwrap().retain(|b| b.id != id);
    changed(&app);
}
