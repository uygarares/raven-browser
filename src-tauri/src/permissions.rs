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
