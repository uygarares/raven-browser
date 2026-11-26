//! Raven Shield: blocks ads and trackers, like Brave Shields. It uses Brave's
//! open source blocking engine (adblock-rust) with the EasyList, EasyPrivacy
//! and uBlock filter lists, downloaded once a week. Every request a page makes
//! is checked; matches get an empty answer. Ad boxes the lists know are hidden
//! with CSS. The shield can be turned off per site.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Webview};

use crate::{native, settings::SettingsState, storage, tabs, windows};

const FILE: &str = "shield.json";
const LISTS: &[(&str, &str)] = &[
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
    ("ublock-filters.txt", "https://ublockorigin.github.io/uAssets/filters/filters.txt"),
    ("ublock-privacy.txt", "https://ublockorigin.github.io/uAssets/filters/privacy.txt"),
];
/// "We use cookies" banners (EasyList Cookie), when that setting is on.
const COOKIE_LIST: (&str, &str) = ("easylist-cookie.txt", "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt");
/// Lists older than this are downloaded again.
const REFRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    /// Sites (host names) where the shield is off.
    allowed: Vec<String>,
    /// Ads and trackers blocked since Raven was installed.
    total: u64,
}

#[derive(Default)]
pub struct Shield {
    engine: RwLock<Option<Arc<Engine>>>,
    allowed: Mutex<HashSet<String>>,
    /// Blocked on the page each tab shows now.
    counts: Mutex<HashMap<u32, u32>>,
    /// Tabs whose count changed since the toolbar was told.
    changed: Mutex<HashSet<u32>>,
    total: AtomicU64,
    /// Why the lists could not be loaded, if they could not.
    error: Mutex<Option<String>>,
}

pub fn load(app: &AppHandle) -> Shield {
    let saved: Saved = storage::read(app, FILE);
    Shield {
        allowed: Mutex::new(saved.allowed.into_iter().collect()),
        total: AtomicU64::new(saved.total),
        ..Default::default()
    }
}

fn save(app: &AppHandle) {
    let shield = app.state::<Shield>();
    let mut allowed: Vec<String> = shield.allowed.lock().unwrap().iter().cloned().collect();
    allowed.sort();
    let saved = Saved { allowed, total: shield.total.load(Ordering::Relaxed) };
    let _ = storage::write(app, FILE, &saved);
}

/// "www.youtube.com" and "m.youtube.com" count as "youtube.com".
pub fn site_of(url: &str) -> String {
    url.parse::<tauri::Url>()
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
        .unwrap_or_default()
}

/// Builds the engine from list texts.
pub fn build(lists: &[String]) -> Engine {
    let mut set = FilterSet::new(false);
    for text in lists {
        set.add_filter_list(text.clone(), ParseOptions::default());
    }
    Engine::new_with_filter_set(set)
}

/// Whether a request should be blocked. Pure (given an engine), for tests.
pub fn blocks(engine: &Engine, url: &str, source: &str, kind: &str) -> bool {
    match Request::new(url, source, kind, "GET") {
        Ok(request) => engine.check_network_request(&request).should_block(),
        Err(_) => false,
    }
}

fn shield_on(app: &AppHandle) -> bool {
    app.state::<SettingsState>().0.lock().unwrap().shield
}

/// Query parameters that only exist to follow you from site to site.
const TRACKING: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "mc_cid", "mc_eid", "igshid", "yclid",
    "_hsenc", "_hsmi", "mkt_tok", "oly_anon_id", "oly_enc_id", "vero_id", "rb_clickid", "s_cid", "ttclid",
    "twclid", "srsltid", "__s", "_openstat", "wickedid", "ml_subscriber", "ml_subscriber_hash",
];

/// The address without tracking parameters (utm_source, fbclid...), or None
/// when there is nothing to remove.
pub fn clean_url(url: &tauri::Url) -> Option<tauri::Url> {
    if !matches!(url.scheme(), "http" | "https") || url.query().is_none() {
        return None;
    }
    let tracking = |name: &str| {
        let name = name.to_ascii_lowercase();
        name.starts_with("utm_") || TRACKING.contains(&name.as_str())
    };
    let pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if !pairs.iter().any(|(k, _)| tracking(k)) {
        return None;
    }
    let mut clean = url.clone();
    let kept: Vec<&(String, String)> = pairs.iter().filter(|(k, _)| !tracking(k)).collect();
    if kept.is_empty() {
        clean.set_query(None);
    } else {
        clean.query_pairs_mut().clear().extend_pairs(kept.iter().map(|(k, v)| (k, v)));
    }
    Some(clean)
}

/// Removing tracking parameters is part of the shield.
pub fn cleans_urls(app: &AppHandle) -> bool {
    shield_on(app)
}

/// Checks one request of tab `id`. Called on the main thread for every request.
fn check(app: &AppHandle, id: u32, url: &str, source: &str, kind: &str) -> bool {
    let shield = app.state::<Shield>();
    let Some(engine) = shield.engine.read().unwrap().clone() else { return false };
    if !shield_on(app) {
        return false;
    }
    let site = site_of(source);
    if shield.allowed.lock().unwrap().contains(&site) {
        return false;
    }
    if !blocks(&engine, url, source, kind) {
        return false;
    }
    *shield.counts.lock().unwrap().entry(id).or_insert(0) += 1;
    shield.changed.lock().unwrap().insert(id);
    shield.total.fetch_add(1, Ordering::Relaxed);
    true
}

/// Starts checking a tab's requests (when its page is created).
pub fn attach(app: &AppHandle, webview: &Webview, id: u32) {
    let app = app.clone();
    native::watch_requests(webview, move |url, source, kind| check(&app, id, url, source, kind));
}

/// A tab started loading a new page: its count starts again.
pub fn page_started(app: &AppHandle, id: u32) {
    let shield = app.state::<Shield>();
    if shield.counts.lock().unwrap().remove(&id).is_some() {
        shield.changed.lock().unwrap().insert(id);
    }
}

/// CSS that hides the ad boxes the lists know for this page, as a script.
pub fn cosmetic_script(app: &AppHandle, url: &str) -> Option<String> {
    if !shield_on(app) || !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let shield = app.state::<Shield>();
    if shield.allowed.lock().unwrap().contains(&site_of(url)) {
        return None;
    }
    let engine = shield.engine.read().unwrap().clone()?;
    let resources = engine.url_cosmetic_resources(url);
    if resources.hide_selectors.is_empty() {
        return None;
    }
    let selectors: Vec<&str> = resources.hide_selectors.iter().map(String::as_str).collect();
    let css = format!("{}{{display:none!important}}", selectors.join(","));
    let css = serde_json::to_string(&css).ok()?;
    Some(format!(
        r#"(() => {{ if (document.getElementById("raven-shield-css")) return; const s = document.createElement("style"); s.id = "raven-shield-css"; s.textContent = {css}; (document.head || document.documentElement).appendChild(s); }})()"#
    ))
}

fn lists_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("shield"))
}

/// Loads the lists from disk (downloading missing or old ones) and builds the engine.
fn prepare(app: &AppHandle) -> Result<(), String> {
    let dir = lists_dir(app).ok_or("No data folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut texts = Vec::new();
    let mut failures = Vec::new();
    let cookies = app.state::<SettingsState>().0.lock().unwrap().hide_cookie_banners;
    let lists = LISTS.iter().chain(cookies.then_some(&COOKIE_LIST));
    for (file, url) in lists {
        let path = dir.join(file);
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|time| SystemTime::now().duration_since(time).ok())
            .is_some_and(|age| age < REFRESH);
        if !fresh {
            match crate::extensions::fetch(url) {
                Ok(bytes) if bytes.len() > 1000 => {
                    let _ = std::fs::write(&path, &bytes);
                }
                Ok(_) | Err(_) => failures.push(*file),
            }
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            texts.push(text);
        }
    }
    if texts.is_empty() {
        return Err(format!("Engelleme listeleri indirilemedi ({})", failures.join(", ")));
    }
    let engine = build(&texts);
    *app.state::<Shield>().engine.write().unwrap() = Some(Arc::new(engine));
    Ok(())
}

/// Builds the engine again (the cookie banner setting changed).
pub fn rebuild(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let result = prepare(&app);
        *app.state::<Shield>().error.lock().unwrap() = result.err();
        let _ = app.emit("shield-ready", ());
    });
}

/// Loads the shield in the background and tells toolbars about new counts.
pub fn start(app: &AppHandle) {
    let loader = app.clone();
    std::thread::spawn(move || {
        let result = prepare(&loader);
        *loader.state::<Shield>().error.lock().unwrap() = result.err();
        let _ = loader.emit("shield-ready", ());
    });
    let ticker = app.clone();
    std::thread::spawn(move || {
        let mut since_save = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(700));
            let changed: Vec<u32> = ticker.state::<Shield>().changed.lock().unwrap().drain().collect();
            if changed.is_empty() {
                continue;
            }
            let counts = ticker.state::<Shield>().counts.lock().unwrap().clone();
            let list: Vec<(u32, u32)> = changed.iter().map(|id| (*id, counts.get(id).copied().unwrap_or(0))).collect();
            let _ = ticker.emit("shield-counts", list);
            since_save += 1;
            if since_save >= 20 {
                since_save = 0;
                save(&ticker);
            }
        }
    });
}

pub fn flush(app: &AppHandle) {
    save(app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShieldInfo {
    /// The shield setting.
    on: bool,
    /// The lists are loaded.
    ready: bool,
    error: Option<String>,
    site: String,
    /// The shield works on this site (not turned off for it).
    site_on: bool,
    /// Blocked on this page.
    page: u32,
    total: u64,
}

#[tauri::command]
pub async fn get_shield(app: AppHandle, webview: Webview) -> ShieldInfo {
    let window = windows::window_of(&webview);
    let (url, _) = tabs::active_page(&app, &window).unwrap_or_default();
    let id = tabs::active_tab_id(&app, &window);
    let shield = app.state::<Shield>();
    let site = site_of(&url);
    let ready = shield.engine.read().unwrap().is_some();
    let error = shield.error.lock().unwrap().clone();
    let site_on = !shield.allowed.lock().unwrap().contains(&site);
    let page = id.and_then(|id| shield.counts.lock().unwrap().get(&id).copied()).unwrap_or(0);
    let total = shield.total.load(Ordering::Relaxed);
    ShieldInfo { on: shield_on(&app), ready, error, site, site_on, page, total }
}

/// Turns the shield on or off for the site of the active tab, and reloads it.
#[tauri::command]
pub async fn set_shield_for_site(app: AppHandle, webview: Webview, on: bool) -> Result<(), String> {
    let window = windows::window_of(&webview);
    let (url, _) = tabs::active_page(&app, &window).ok_or("No page")?;
    let site = site_of(&url);
    if site.is_empty() {
        return Err("No site".into());
    }
    {
        let shield = app.state::<Shield>();
        let mut allowed = shield.allowed.lock().unwrap();
        if on {
            allowed.remove(&site);
        } else {
            allowed.insert(site);
        }
    }
    save(&app);
    if let Some(tab) = tabs::active_webview(&app, &window) {
        let _ = tab.reload();
    }
    let _ = app.emit("shield-ready", ());
    Ok(())
}

/// Downloads the lists again now.
#[tauri::command]
pub async fn update_shield_lists(app: AppHandle) -> Result<(), String> {
    if let Some(dir) = lists_dir(&app) {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let result = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || prepare(&app)
    })
    .await
    .map_err(|e| e.to_string())?;
    *app.state::<Shield>().error.lock().unwrap() = result.clone().err();
    let _ = app.emit("shield-ready", ());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_listed_ads_but_not_pages() {
        let engine = build(&["||doubleclick.net^\n||ads.example.com^$third-party\n@@||ads.example.com/ok.js".to_string()]);
        assert!(blocks(&engine, "https://ad.doubleclick.net/x.js", "https://news.com/", "script"));
        assert!(blocks(&engine, "https://ads.example.com/a.png", "https://news.com/", "image"));
        assert!(!blocks(&engine, "https://ads.example.com/ok.js", "https://news.com/", "script"));
        assert!(!blocks(&engine, "https://news.com/app.js", "https://news.com/", "script"));
    }

    #[test]
    fn removes_tracking_parameters() {
        let clean = |u: &str| clean_url(&u.parse().unwrap()).map(|u| u.to_string());
        assert_eq!(clean("https://a.com/x?utm_source=tw&id=5&fbclid=abc").as_deref(), Some("https://a.com/x?id=5"));
        assert_eq!(clean("https://a.com/x?UTM_medium=1").as_deref(), Some("https://a.com/x"));
        assert_eq!(clean("https://a.com/x?id=5"), None);
        assert_eq!(clean("https://a.com/x"), None);
    }

    #[test]
    fn sites_drop_www() {
        assert_eq!(site_of("https://www.youtube.com/watch?v=1"), "youtube.com");
        assert_eq!(site_of("raven"), "");
    }
}
