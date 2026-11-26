//! Raven's memory saving, checked every 30 seconds:
//! - tabs you have not looked at for a while go to sleep (their page closes and
//!   loads again when you open them),
//! - above the RAM limit the longest unused tabs sleep first,
//! - tabs not opened for days move to the archive,
//! - game mode: while a full screen game runs, background tabs sleep.
//! The tab you look at, tabs playing sound and pinned tabs never sleep.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{native, settings::SettingsState, storage, tabs};

const ARCHIVE_FILE: &str = "archive.json";
static GAME_MODE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Archived {
    pub url: String,
    pub title: String,
    /// When it was archived, seconds since 1970.
    pub time: u64,
}

#[derive(Default)]
pub struct Archive(Mutex<Vec<Archived>>);

pub fn load(app: &AppHandle) -> Archive {
    Archive(Mutex::new(storage::read(app, ARCHIVE_FILE)))
}

fn save_archive(app: &AppHandle) {
    let list = app.state::<Archive>().0.lock().unwrap().clone();
    let _ = storage::write(app, ARCHIVE_FILE, &list);
    let _ = app.emit("archive", ());
}

/// Which tabs to put to sleep, oldest first. Pure, so it can be tested.
pub fn sleep_candidates(tabs: &[tabs::TabUse], older_than: Option<Duration>) -> Vec<u32> {
    let mut list: Vec<&tabs::TabUse> = tabs
        .iter()
        .filter(|t| !t.active && !t.asleep && !t.pinned && !t.audible)
        .filter(|t| older_than.is_none_or(|limit| t.idle >= limit))
        .collect();
    list.sort_by(|a, b| b.idle.cmp(&a.idle));
    list.into_iter().map(|t| t.id).collect()
}

/// Tabs not opened for `days` days (never pinned, active or private ones).
pub fn archive_candidates(tabs: &[tabs::TabUse], days: u32, now: u64) -> Vec<u32> {
    let limit = u64::from(days) * 24 * 60 * 60;
    tabs.iter()
        .filter(|t| !t.active && !t.pinned && !t.private && !t.audible && !t.url.is_empty())
        .filter(|t| now.saturating_sub(t.last_used) >= limit)
        .map(|t| t.id)
        .collect()
}

/// A full screen game (or other full screen program) is in front.
#[cfg(windows)]
fn game_running(app: &AppHandle) -> bool {
    use ::windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN};
    // Raven itself in full screen (a video) is not a game.
    let raven_in_front = app.webview_windows().values().any(|w| w.is_focused().unwrap_or(false))
        || app.windows().values().any(|w| w.is_focused().unwrap_or(false));
    if raven_in_front {
        return false;
    }
    matches!(unsafe { SHQueryUserNotificationState() }, Ok(state) if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_BUSY)
}

#[cfg(not(windows))]
fn game_running(_app: &AppHandle) -> bool {
    false
}

pub fn game_mode_on() -> bool {
    GAME_MODE.load(Ordering::Relaxed)
}

/// Measures memory now (MB): total and per tab. Saves it for hover cards.
pub fn measure(app: &AppHandle) -> Option<u64> {
    let ui = app.webviews().into_iter().find(|(label, _)| label.starts_with("ui-")).map(|(_, w)| w)?;
    let memory = native::memory(&ui).ok()?;
    tabs::set_memory(app, &memory.pages);
    Some(memory.total / 1_048_576)
}

fn check(app: &AppHandle) {
    let settings = app.state::<SettingsState>().0.lock().unwrap().clone();
    let all = tabs::all_tabs(app);

    // Game mode.
    let game = settings.game_mode && game_running(app);
    if game != GAME_MODE.swap(game, Ordering::Relaxed) {
        let _ = app.emit("game-mode", game);
    }
    if game {
        for id in sleep_candidates(&all, None) {
            tabs::sleep_tab(app, id);
        }
        return;
    }

    // Sleep after a while in the background.
    if settings.sleep_minutes > 0 {
        let limit = Duration::from_secs(u64::from(settings.sleep_minutes) * 60);
        for id in sleep_candidates(&all, Some(limit)) {
            tabs::sleep_tab(app, id);
        }
    }

    // Memory limit: put the longest unused tabs to sleep, a few at a time.
    let total = measure(app);
    if let (Some(total), limit) = (total, settings.memory_limit_mb) {
        if limit > 0 && total > u64::from(limit) {
            let all = tabs::all_tabs(app);
            for id in sleep_candidates(&all, Some(Duration::from_secs(60))).into_iter().take(2) {
                tabs::sleep_tab(app, id);
            }
        }
    }

    // Archive old tabs.
    if settings.archive_days > 0 {
        let all = tabs::all_tabs(app);
        let old = archive_candidates(&all, settings.archive_days, tabs::now_secs());
        if !old.is_empty() {
            {
                let state = app.state::<Archive>();
                let mut archive = state.0.lock().unwrap();
                for id in &old {
                    if let Some(tab) = all.iter().find(|t| t.id == *id) {
                        archive.insert(0, Archived { url: tab.url.clone(), title: tab.title.clone(), time: tabs::now_secs() });
                    }
                }
                archive.truncate(500);
            }
            save_archive(app);
            for id in old {
                let _ = tabs::close(app, id);
            }
        }
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        check(&app);
    });
}

#[tauri::command]
pub async fn get_archive(app: AppHandle) -> Vec<Archived> {
    app.state::<Archive>().0.lock().unwrap().clone()
}

/// Opens an archived tab again (and takes it out of the archive).
#[tauri::command]
pub async fn open_archived(app: AppHandle, webview: tauri::Webview, index: usize) -> Result<(), String> {
    let item = {
        let state = app.state::<Archive>();
        let mut archive = state.0.lock().unwrap();
        if index >= archive.len() {
            return Err("Not found".into());
        }
        archive.remove(index)
    };
    save_archive(&app);
    tabs::open_tab(&app, &crate::windows::window_of(&webview), &item.url).map(|_| ())
}

#[tauri::command]
pub async fn clear_archive(app: AppHandle) {
    app.state::<Archive>().0.lock().unwrap().clear();
    save_archive(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    total_mb: Option<u64>,
    awake: usize,
    asleep: usize,
    game_mode: bool,
}

/// RAM now, for the settings page and the menu.
#[tauri::command]
pub async fn get_memory(app: AppHandle) -> MemoryInfo {
    let total_mb = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || measure(&app)
    })
    .await
    .ok()
    .flatten();
    let all = tabs::all_tabs(&app);
    MemoryInfo {
        total_mb,
        awake: all.iter().filter(|t| !t.asleep).count(),
        asleep: all.iter().filter(|t| t.asleep).count(),
        game_mode: game_mode_on(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u32, idle: u64) -> tabs::TabUse {
        tabs::TabUse {
            id,
            window: "win-1".into(),
            url: format!("https://{id}.com/"),
            title: String::new(),
            active: false,
            asleep: false,
            pinned: false,
            audible: false,
            private: false,
            idle: Duration::from_secs(idle),
            last_used: 0,
        }
    }

    #[test]
    fn sleeps_oldest_first_and_spares_important_tabs() {
        let mut tabs = vec![tab(1, 100), tab(2, 900), tab(3, 2000), tab(4, 5000), tab(5, 9000)];
        tabs[2].active = true;
        tabs[3].pinned = true;
        tabs[4].audible = true;
        assert_eq!(sleep_candidates(&tabs, Some(Duration::from_secs(600))), vec![2]);
        assert_eq!(sleep_candidates(&tabs, None), vec![2, 1]);
    }

    #[test]
    fn archives_old_tabs() {
        let mut old = tab(1, 0);
        old.last_used = 0;
        let mut fresh = tab(2, 0);
        fresh.last_used = 10 * 86_400;
        assert_eq!(archive_candidates(&[old, fresh], 7, 10 * 86_400), vec![1]);
    }
}
