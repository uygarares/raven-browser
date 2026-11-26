//! Import from other browsers on this computer: Chrome, Edge, Brave, Opera and
//! Vivaldi keep bookmarks in a JSON file named "Bookmarks" in their profile.

use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use crate::bookmarks;
use crate::sqlite_reader::Database;

/// id, name, profile folder under %LOCALAPPDATA% (or %APPDATA% for Opera).
const SOURCES: &[(&str, &str, &str, bool)] = &[
    ("chrome", "Chrome", r"Google\Chrome\User Data\Default", false),
    ("edge", "Edge", r"Microsoft\Edge\User Data\Default", false),
    ("brave", "Brave", r"BraveSoftware\Brave-Browser\User Data\Default", false),
    ("vivaldi", "Vivaldi", r"Vivaldi\User Data\Default", false),
    ("opera", "Opera", r"Opera Software\Opera Stable", true),
];

fn profile_dir(local: &str, roaming: bool) -> Option<PathBuf> {
    let base = std::env::var_os(if roaming { "APPDATA" } else { "LOCALAPPDATA" })?;
    Some(PathBuf::from(base).join(local.replace('\\', std::path::MAIN_SEPARATOR_STR)))
}

/// Every bookmark (title, address) in a Chromium "Bookmarks" file, folders
/// flattened, bookmarks bar first.
pub fn read_bookmarks(json: &str) -> Vec<(String, String)> {
    fn walk(node: &serde_json::Value, out: &mut Vec<(String, String)>) {
        match node["type"].as_str() {
            Some("url") => {
                if let (Some(name), Some(url)) = (node["name"].as_str(), node["url"].as_str()) {
                    if url.starts_with("http://") || url.starts_with("https://") {
                        out.push((name.to_string(), url.to_string()));
                    }
                }
            }
            _ => {
                if let Some(children) = node["children"].as_array() {
                    for child in children {
                        walk(child, out);
                    }
                }
            }
        }
    }
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let mut out = Vec::new();
    for key in ["bookmark_bar", "other", "synced"] {
        walk(&root["roots"][key], &mut out);
    }
    out
}

#[derive(Serialize)]
pub struct Source {
    id: &'static str,
    name: &'static str,
}

/// The browsers found on this computer.
#[tauri::command]
pub async fn get_import_sources() -> Vec<Source> {
    SOURCES
        .iter()
        .filter(|(_, _, dir, roaming)| profile_dir(dir, *roaming).is_some_and(|d| d.join("Bookmarks").exists()))
        .map(|(id, name, _, _)| Source { id, name })
        .collect()
}

#[derive(Serialize)]
pub struct Imported {
    bookmarks: usize,
    history: usize,
}

/// Chromium's history file ("History", SQLite): the most visited and most
/// recent pages, newest first. Times are microseconds since 1601.
pub fn read_history(bytes: &[u8], limit: usize) -> Result<Vec<(String, String, i64, i64)>, String> {
    let db = Database::open(bytes)?;
    let root = db.table_root("urls").ok_or("Geçmiş tablosu bulunamadı")?;
    let mut rows: Vec<(String, String, i64, i64)> = db
        .rows(root)
        .into_iter()
        .filter(|row| row.len() >= 6 && row.get(6).map_or(0, |h| h.as_int()) == 0)
        .filter_map(|row| {
            let url = row[1].as_text().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return None;
            }
            let ms = row[5].as_int() / 1000 - 11_644_473_600_000;
            Some((url, row[2].as_text().to_string(), ms, row[3].as_int()))
        })
        .collect();
    rows.sort_by(|a, b| b.2.cmp(&a.2));
    rows.truncate(limit);
    Ok(rows)
}

#[tauri::command]
pub async fn import_browser(app: AppHandle, id: String) -> Result<Imported, String> {
    let (_, name, dir, roaming) = SOURCES.iter().find(|(s, _, _, _)| *s == id).ok_or("Bilinmeyen tarayıcı")?;
    let dir = profile_dir(dir, *roaming).ok_or("Klasör bulunamadı")?;
    let text = std::fs::read_to_string(dir.join("Bookmarks")).map_err(|_| format!("{name} yer imleri okunamadı"))?;
    let items = read_bookmarks(&text);
    let added = bookmarks::add_many(&app, items);
    // History: copied first, the browser may have it open.
    let history = std::fs::read(dir.join("History"))
        .ok()
        .and_then(|bytes| read_history(&bytes, 4000).ok())
        .map(|visits| crate::history::import(&app, visits))
        .unwrap_or(0);
    Ok(Imported { bookmarks: added, history })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_chrome_history() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/history.sqlite");
        let Ok(bytes) = std::fs::read(path) else { return };
        let visits = read_history(&bytes, 10).unwrap();
        assert_eq!(visits.len(), 10);
        assert_eq!(visits[0].0, "https://example.com/page/3000");
        // 13370000000003000 µs since 1601 = 1725526400003 ms since 1970.
        assert_eq!(visits[0].2, 13_370_000_000_003_000 / 1000 - 11_644_473_600_000);
    }

    #[test]
    fn reads_nested_bookmarks() {
        let json = r#"{"roots":{
          "bookmark_bar":{"type":"folder","children":[
            {"type":"url","name":"GitHub","url":"https://github.com/"},
            {"type":"folder","name":"İş","children":[{"type":"url","name":"Mail","url":"https://mail.google.com/"}]},
            {"type":"url","name":"JS","url":"javascript:alert(1)"}]},
          "other":{"type":"folder","children":[{"type":"url","name":"Wiki","url":"https://tr.wikipedia.org/"}]}}}"#;
        let list = read_bookmarks(json);
        assert_eq!(list.len(), 3);
        assert_eq!(list[1], ("Mail".to_string(), "https://mail.google.com/".to_string()));
    }
}
