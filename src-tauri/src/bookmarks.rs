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

/// Bookmarks in the HTML format every browser can export and import
/// ("Netscape bookmark file"): (title, address) pairs, folders flattened.
pub fn parse_html(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<a ").map(|i| at + i) {
        let Some(tag_end) = lower[start..].find('>').map(|i| start + i) else { break };
        let Some(close) = lower[tag_end..].find("</a>").map(|i| tag_end + i) else { break };
        let tag = &text[start..tag_end];
        let href = tag
            .to_ascii_lowercase()
            .find("href=\"")
            .and_then(|i| tag[i + 6..].split('"').next())
            .map(unescape)
            .unwrap_or_default();
        let title = unescape(text[tag_end + 1..close].trim());
        if href.starts_with("http://") || href.starts_with("https://") {
            found.push((if title.is_empty() { href.clone() } else { title }, href));
        }
        at = close + 4;
    }
    found
}

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The bookmarks as an HTML file for other browsers.
pub fn to_html(bookmarks: &[Bookmark]) -> String {
    let mut out = String::from("<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n    <DT><H3 PERSONAL_TOOLBAR_FOLDER=\"true\">Raven</H3>\n    <DL><p>\n");
    for bookmark in bookmarks {
        out.push_str(&format!("        <DT><A HREF=\"{}\">{}</A>\n", escape(&bookmark.url), escape(&bookmark.title)));
    }
    out.push_str("    </DL><p>\n</DL><p>\n");
    out
}

/// Saves the bookmarks to Downloads as an HTML file. Returns the file name.
#[tauri::command]
pub async fn export_bookmarks_html(app: AppHandle) -> Result<String, String> {
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let name = "Raven yer imleri.html".to_string();
    std::fs::write(dir.join(&name), to_html(&list(&app))).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Adds the bookmarks of an HTML file (from Chrome, Firefox, Edge, Safari...).
#[tauri::command]
pub async fn import_bookmarks_html(app: AppHandle, text: String) -> Result<usize, String> {
    let items = parse_html(&text);
    if items.is_empty() {
        return Err("Bu dosyada yer imi bulunamadı".into());
    }
    Ok(add_many(&app, items))
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

/// Renames a bookmark or changes its address (bookmarks page).
#[tauri::command]
pub async fn edit_bookmark(app: AppHandle, id: i64, title: String, url: String) -> Result<(), String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("Adres boş olamaz".into());
    }
    {
        let state = app.state::<Bookmarks>();
        let mut list = state.0.lock().unwrap();
        let bookmark = list.iter_mut().find(|b| b.id == id).ok_or("Yer imi bulunamadı")?;
        bookmark.title = if title.trim().is_empty() { url.clone() } else { title.trim().to_string() };
        bookmark.url = url;
    }
    changed(&app);
    Ok(())
}

/// Moves a bookmark to another place in the list (and the bar).
#[tauri::command]
pub async fn move_bookmark(app: AppHandle, id: i64, index: usize) {
    {
        let state = app.state::<Bookmarks>();
        let mut list = state.0.lock().unwrap();
        if let Some(from) = list.iter().position(|b| b.id == id) {
            let item = list.remove(from);
            let to = index.min(list.len());
            list.insert(to, item);
        }
    }
    changed(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_bookmark_files() {
        let html = r#"<DL><p><DT><H3>Bar</H3><DL><p><DT><A HREF="https://github.com/" ADD_DATE="1">GitHub &amp; co</A>
<DT><A href="javascript:alert(1)">x</A><DT><A HREF="https://f2pclub.app">F2P Club</A></DL></DL>"#;
        assert_eq!(
            parse_html(html),
            vec![("GitHub & co".to_string(), "https://github.com/".to_string()), ("F2P Club".to_string(), "https://f2pclub.app".to_string())]
        );
        let out = to_html(&[Bookmark { id: 1, title: "A <b>".into(), url: "https://a.com/?x=1&y=2".into() }]);
        assert_eq!(parse_html(&out), vec![("A <b>".to_string(), "https://a.com/?x=1&y=2".to_string())]);
    }
}
