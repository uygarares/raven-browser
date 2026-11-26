//! Import from other browsers on this computer: Chrome, Edge, Brave, Opera and
//! Vivaldi keep bookmarks in a JSON file named "Bookmarks" in their profile.

use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use crate::bookmarks;

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

#[tauri::command]
pub async fn import_browser(app: AppHandle, id: String) -> Result<Imported, String> {
    let (_, name, dir, roaming) = SOURCES.iter().find(|(s, _, _, _)| *s == id).ok_or("Bilinmeyen tarayıcı")?;
    let dir = profile_dir(dir, *roaming).ok_or("Klasör bulunamadı")?;
    let text = std::fs::read_to_string(dir.join("Bookmarks")).map_err(|_| format!("{name} yer imleri okunamadı"))?;
    let items = read_bookmarks(&text);
    let added = bookmarks::add_many(&app, items);
    Ok(Imported { bookmarks: added, history: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

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
