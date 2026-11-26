//! Browsing history, saved to history.json.
//!
//! Pages are recorded when they finish loading. Saving to disk happens a few
//! seconds later in the background (and when Raven closes), not on every page.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::storage;

const FILE: &str = "history.json";
/// Oldest entries are dropped beyond this.
const MAX_ENTRIES: usize = 10_000;
/// The same address visited again within this time updates the last entry.
const SAME_VISIT_MS: i64 = 30 * 60 * 1000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    pub url: String,
    pub title: String,
    /// When it was visited, in milliseconds since 1970.
    pub time: i64,
}

#[derive(Default)]
pub struct History {
    entries: Mutex<Vec<Entry>>,
    dirty: AtomicBool,
}

pub fn load(app: &AppHandle) -> History {
    let entries: Vec<Entry> = storage::read(app, FILE);
    History { entries: Mutex::new(entries), dirty: AtomicBool::new(false) }
}

/// Saves now if anything changed.
pub fn flush(app: &AppHandle) {
    let history = app.state::<History>();
    if !history.dirty.swap(false, Ordering::SeqCst) {
        return;
    }
    let entries = history.entries.lock().unwrap().clone();
    if let Err(error) = storage::write(app, FILE, &entries) {
        eprintln!("Could not save history: {error}");
    }
}

/// Saves changes every few seconds in the background.
pub fn start_saving(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        flush(&app);
    });
}

fn worth_keeping(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Records a visit.
pub fn record(app: &AppHandle, url: &str, title: &str) {
    if !worth_keeping(url) {
        return;
    }
    let history = app.state::<History>();
    let mut entries = history.entries.lock().unwrap();
    let now = storage::now_ms();
    match entries.last_mut() {
        Some(last) if last.url == url && now - last.time < SAME_VISIT_MS => {
            last.time = now;
            if !title.is_empty() {
                last.title = title.to_string();
            }
        }
        _ => entries.push(Entry { url: url.into(), title: title.into(), time: now }),
    }
    if entries.len() > MAX_ENTRIES {
        let extra = entries.len() - MAX_ENTRIES;
        entries.drain(..extra);
    }
    history.dirty.store(true, Ordering::SeqCst);
}

/// Fills in the title of the latest visit to an address.
pub fn set_title(app: &AppHandle, url: &str, title: &str) {
    let history = app.state::<History>();
    let mut entries = history.entries.lock().unwrap();
    if let Some(entry) = entries.iter_mut().rev().take(50).find(|entry| entry.url == url) {
        if entry.title != title {
            entry.title = title.to_string();
            history.dirty.store(true, Ordering::SeqCst);
        }
    }
}

fn matches(entry: &Entry, words: &[String]) -> bool {
    let haystack = format!("{} {}", entry.title, entry.url).to_lowercase();
    words.iter().all(|word| haystack.contains(word))
}

/// Newest first. With a query, only entries whose title or address contain every word.
pub fn search(entries: &[Entry], query: &str, limit: usize) -> Vec<Entry> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
    entries
        .iter()
        .rev()
        .filter(|entry| matches(entry, &words))
        .take(limit)
        .cloned()
        .collect()
}

#[tauri::command]
pub async fn get_history(app: AppHandle, query: String) -> Vec<Entry> {
    let history = app.state::<History>();
    let entries = history.entries.lock().unwrap();
    search(&entries, &query, 500)
}

#[tauri::command]
pub async fn remove_history_entry(app: AppHandle, url: String, time: i64) {
    let history = app.state::<History>();
    history.entries.lock().unwrap().retain(|entry| !(entry.url == url && entry.time == time));
    history.dirty.store(true, Ordering::SeqCst);
    flush(&app);
}

#[tauri::command]
pub async fn clear_history(app: AppHandle) {
    let history = app.state::<History>();
    history.entries.lock().unwrap().clear();
    history.dirty.store(true, Ordering::SeqCst);
    flush(&app);
}

#[cfg(test)]
mod tests {
    use super::{search, Entry};

    fn entry(url: &str, title: &str, time: i64) -> Entry {
        Entry { url: url.into(), title: title.into(), time }
    }

    #[test]
    fn newest_first_and_every_word_must_match() {
        let entries = vec![
            entry("https://github.com/uygarares/raven-browser", "Raven Browser", 1),
            entry("https://www.youtube.com/", "YouTube", 2),
            entry("https://f2pclub.app/", "F2P Club", 3),
        ];
        let all = search(&entries, "", 10);
        assert_eq!(all[0].title, "F2P Club");
        let found = search(&entries, "raven github", 10);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Raven Browser");
        assert!(search(&entries, "RAVEN", 10).len() == 1);
    }
}
