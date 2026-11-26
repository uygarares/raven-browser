//! Site permissions (camera, microphone, location, notifications), saved per
//! website in permissions.json and changed from the site info box (the lock in
//! the address bar), like Chrome's site settings.
//!
//! "Ask" lets WebView2 show its usual question to the user.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::webview::{PermissionKind, PermissionResponse};
use tauri::{AppHandle, Manager, Webview};

use crate::{storage, zoom};

const FILE: &str = "permissions.json";

/// The permissions Raven lets you set per site.
pub const KINDS: [&str; 4] = ["camera", "microphone", "location", "notifications"];

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    #[default]
    Ask,
    Allow,
    Block,
}

/// site -> permission -> choice
type Saved = HashMap<String, HashMap<String, Choice>>;

#[derive(Default)]
pub struct Permissions(Mutex<Saved>);

pub fn load(app: &AppHandle) -> Permissions {
    Permissions(Mutex::new(storage::read(app, FILE)))
}

fn kind_name(kind: &PermissionKind) -> Option<&'static str> {
    match kind {
        PermissionKind::Camera => Some("camera"),
        PermissionKind::Microphone => Some("microphone"),
        PermissionKind::Geolocation => Some("location"),
        PermissionKind::Notifications => Some("notifications"),
        _ => None,
    }
}

fn choice_for(saved: &Saved, site: &str, kind: &str) -> Choice {
    saved.get(site).and_then(|kinds| kinds.get(kind)).copied().unwrap_or_default()
}

/// "Forget this site when I close it" (like Brave): the site's address key.
fn forget_key(url: &str) -> Option<String> {
    zoom::site_of(url).map(|host| host.trim_start_matches("www.").to_string())
}

fn forgets(app: &AppHandle, site: &str) -> bool {
    app.state::<Permissions>().0.lock().unwrap().get(site).and_then(|kinds| kinds.get("forget")).copied() == Some(Choice::Allow)
}

#[tauri::command]
pub async fn get_forget_site(app: AppHandle, url: String) -> bool {
    forget_key(&url).is_some_and(|site| forgets(&app, &site))
}

#[tauri::command]
pub async fn set_forget_site(app: AppHandle, url: String, on: bool) -> Result<(), String> {
    let site = forget_key(&url).ok_or("Not a website")?;
    let saved = {
        let state = app.state::<Permissions>();
        let mut saved = state.0.lock().unwrap();
        let kinds = saved.entry(site.clone()).or_default();
        if on {
            kinds.insert("forget".into(), Choice::Allow);
        } else {
            kinds.remove("forget");
        }
        if kinds.is_empty() {
            saved.remove(&site);
        }
        saved.clone()
    };
    storage::write(&app, FILE, &saved)
}

/// A tab closed: if its site is to be forgotten and no other tab shows it,
/// its cookies and saved data are deleted.
pub fn tab_closed(app: &AppHandle, url: &str) {
    let Some(site) = forget_key(url) else { return };
    if !forgets(app, &site) || crate::tabs::any_tab_on(app, &site) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(webview) = crate::settings::normal_webview(&app) else { return };
        for origin in [format!("https://{site}"), format!("https://www.{site}"), format!("http://{site}"), format!("http://www.{site}")] {
            crate::native::devtools(&webview, "Storage.clearDataForOrigin", serde_json::json!({ "origin": origin, "storageTypes": "all" }));
        }
        for page in [format!("https://{site}/"), format!("https://www.{site}/")] {
            let _ = crate::native::site_cookies(&webview, &page, true);
        }
    });
}

/// Called when a website asks for a permission.
pub fn decide(app: &AppHandle, webview: &Webview, kind: PermissionKind) -> PermissionResponse {
    let Some(name) = kind_name(&kind) else { return PermissionResponse::Default };
    let Some(site) = webview.url().ok().and_then(|url| zoom::site_of(url.as_str())) else {
        return PermissionResponse::Default;
    };
    let saved = app.state::<Permissions>();
    let saved = saved.0.lock().unwrap();
    match choice_for(&saved, &site, name) {
        Choice::Allow => PermissionResponse::Allow,
        Choice::Block => PermissionResponse::Deny,
        Choice::Ask => PermissionResponse::Default,
    }
}

fn save(app: &AppHandle) {
    let saved = app.state::<Permissions>().0.lock().unwrap().clone();
    if let Err(error) = storage::write(app, FILE, &saved) {
        eprintln!("Could not save site permissions: {error}");
    }
}

#[derive(Serialize)]
pub struct SitePermission {
    kind: &'static str,
    choice: Choice,
}

#[tauri::command]
pub async fn get_site_permissions(app: AppHandle, url: String) -> Vec<SitePermission> {
    let site = zoom::site_of(&url).unwrap_or_default();
    let saved = app.state::<Permissions>();
    let saved = saved.0.lock().unwrap();
    KINDS.iter().map(|kind| SitePermission { kind, choice: choice_for(&saved, &site, kind) }).collect()
}

#[tauri::command]
pub async fn set_site_permission(app: AppHandle, url: String, kind: String, choice: Choice) -> Result<(), String> {
    let site = zoom::site_of(&url).ok_or("Not a website")?;
    if !KINDS.contains(&kind.as_str()) {
        return Err("Unknown permission".into());
    }
    {
        let state = app.state::<Permissions>();
        let mut saved = state.0.lock().unwrap();
        let kinds = saved.entry(site.clone()).or_default();
        if choice == Choice::Ask {
            kinds.remove(&kind);
        } else {
            kinds.insert(kind, choice);
        }
        if kinds.is_empty() {
            saved.remove(&site);
        }
    }
    save(&app);
    Ok(())
}

#[tauri::command]
pub async fn reset_site_permissions(app: AppHandle, url: String) {
    if let Some(site) = zoom::site_of(&url) {
        app.state::<Permissions>().0.lock().unwrap().remove(&site);
        save(&app);
    }
}

#[cfg(test)]
mod tests {
    use super::{choice_for, Choice, Saved};

    #[test]
    fn unknown_sites_ask() {
        let mut saved = Saved::new();
        saved.entry("meet.google.com".into()).or_default().insert("camera".into(), Choice::Allow);
        assert_eq!(choice_for(&saved, "meet.google.com", "camera"), Choice::Allow);
        assert_eq!(choice_for(&saved, "meet.google.com", "microphone"), Choice::Ask);
        assert_eq!(choice_for(&saved, "example.com", "camera"), Choice::Ask);
    }
}

/// Cookies of the active tab's site (site info box).
#[tauri::command]
pub async fn get_site_cookies(app: AppHandle, webview: tauri::Webview) -> Result<usize, String> {
    let window = crate::windows::window_of(&webview);
    let tab = crate::tabs::active_webview(&app, &window).ok_or("No page")?;
    let (url, _) = crate::tabs::active_page(&app, &window).ok_or("No page")?;
    tauri::async_runtime::spawn_blocking(move || crate::native::site_cookies(&tab, &url, false))
        .await
        .map_err(|e| e.to_string())?
}

/// Deletes the active site's cookies and stored data, then reloads it.
#[tauri::command]
pub async fn clear_site_data(app: AppHandle, webview: tauri::Webview) -> Result<usize, String> {
    let window = crate::windows::window_of(&webview);
    let tab = crate::tabs::active_webview(&app, &window).ok_or("No page")?;
    let (url, _) = crate::tabs::active_page(&app, &window).ok_or("No page")?;
    let worker = tab.clone();
    let count = tauri::async_runtime::spawn_blocking(move || crate::native::site_cookies(&worker, &url, true))
        .await
        .map_err(|e| e.to_string())??;
    let _ = tab.eval("try { localStorage.clear(); sessionStorage.clear(); } catch (e) {} location.reload();");
    Ok(count)
}
