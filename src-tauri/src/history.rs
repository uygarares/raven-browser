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

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TopSite {
    pub url: String,
    pub title: String,
}

/// The most visited sites, one per website, most visited first. Used for the
/// shortcuts on the new tab page until the user picks their own.
pub fn top_sites(entries: &[Entry], limit: usize) -> Vec<TopSite> {
    use std::collections::HashMap;
    // site -> (visits, latest visit, a title)
    let mut sites: HashMap<String, (usize, i64, String)> = HashMap::new();
    for entry in entries {
        let Ok(url) = entry.url.parse::<tauri::Url>() else { continue };
        let Some(host) = url.host_str() else { continue };
        let home = format!("{}://{}/", url.scheme(), host);
        let site = sites.entry(home).or_insert((0, 0, String::new()));
        site.0 += 1;
        if entry.time >= site.1 {
            site.1 = entry.time;
        }
        // Prefer the title of the home page, else any title.
        let is_home = url.path() == "/" && url.query().is_none();
        if !entry.title.is_empty() && (is_home || site.2.is_empty()) {
            site.2 = entry.title.clone();
        }
    }
    let mut ranked: Vec<_> = sites.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(b.1 .1.cmp(&a.1 .1)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(url, (_, _, title))| TopSite { title: short_title(&title, &url), url })
        .collect()
}

/// "YouTube - Broadcast Yourself" becomes "YouTube"; no title becomes the site name.
fn short_title(title: &str, url: &str) -> String {
    let first = title
        .split([' ', ' '])
        .collect::<Vec<_>>()
        .join(" ");
    let first = first
        .split(" - ")
        .next()
        .unwrap_or("")
        .split(" | ")
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if !first.is_empty() {
        return first;
    }
    url.parse::<tauri::Url>()
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_top_sites(app: AppHandle) -> Vec<TopSite> {
    let history = app.state::<History>();
    let entries = history.entries.lock().unwrap();
    top_sites(&entries, 8)
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
    fn top_sites_counts_visits_per_site() {
        let entries = vec![
            entry("https://www.youtube.com/watch?v=1", "Video - YouTube", 1),
            entry("https://www.youtube.com/", "YouTube", 2),
            entry("https://github.com/x", "x · GitHub", 3),
            entry("https://www.youtube.com/watch?v=2", "Other - YouTube", 4),
        ];
        let top = super::top_sites(&entries, 8);
        assert_eq!(top[0].url, "https://www.youtube.com/");
        assert_eq!(top[0].title, "YouTube");
        assert_eq!(top[1].url, "https://github.com/");
        assert_eq!(top.len(), 2);
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
