//! Tabs: each awake tab is its own webview inside its window.
//!
//! A tab can also be asleep: it is listed in the tab row, but has no webview and
//! uses no memory until you click it. Tabs restored from the last session start
//! asleep (except each window's active one).

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl,
};

use crate::{downloads, extensions, history, media, permissions, session, settings, windows, zoom};

pub const HOME_URL: &str = "https://www.google.com";
/// Opens Raven's own new tab page (ui/newtab.html).
pub const NEW_TAB: &str = "raven:newtab";
const NEW_TAB_PAGE: &str = "newtab.html";
const WHITE: tauri::window::Color = tauri::window::Color(255, 255, 255, 255);
const NEW_TAB_TITLE: &str = "Yeni sekme";
const MAX_CLOSED: usize = 25;

#[derive(Clone, Serialize)]
pub struct TabInfo {
    id: u32,
    #[serde(skip)]
    window: String,
    title: String,
    /// Empty for Raven's new tab page.
    url: String,
    loading: bool,
    /// No webview yet: the page loads when the tab is opened.
    asleep: bool,
    /// In a private window: not saved in history or the session.
    private: bool,
    /// Page zoom, 1.0 = 100%.
    zoom: f64,
    /// Pinned tabs stay small at the left of the tab row.
    pinned: bool,
    /// The page is playing sound.
    audible: bool,
    muted: bool,
    /// Memory of the tab's page in MB, from the last measurement (hover card).
    memory: Option<u32>,
    /// When the tab was last shown (for sleeping tabs).
    #[serde(skip)]
    last_seen: std::time::Instant,
    /// When the tab was last opened, in seconds since 1970 (for the archive).
    #[serde(skip)]
    last_used: u64,
    /// The tab group it belongs to (Chrome-like coloured groups).
    group: Option<u32>,
}

/// A named, coloured group of tabs that can be folded (collapsed).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TabGroup {
    pub id: u32,
    pub name: String,
    /// One of GROUP_COLORS.
    pub color: String,
    pub collapsed: bool,
}

pub const GROUP_COLORS: &[&str] = &["blue", "violet", "pink", "red", "orange", "yellow", "green", "cyan", "grey"];

/// Decodes %XX escapes (UTF-8) in text from a raven-action: address.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// http:// becomes https:// ("Her zaman güvenli bağlantı"), except for this
/// computer and home networks, which often have no https.
pub fn upgrade(mut url: Url) -> Url {
    if url.scheme() != "http" {
        return url;
    }
    let host = url.host_str().unwrap_or("");
    let is_ip = host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[');
    let local = is_ip || host == "localhost" || host.ends_with(".local") || !host.contains('.');
    if !local {
        let _ = url.set_scheme("https");
    }
    url
}

/// Puts the biggest video of the page in a floating window, or takes it back.
pub const PIP_SCRIPT: &str = r#"(() => { if (document.pictureInPictureElement) return document.exitPictureInPicture(); const v = [...document.querySelectorAll("video")].sort((a, b) => (b.clientWidth * b.clientHeight) - (a.clientWidth * a.clientHeight))[0]; if (v) { v.disablePictureInPicture = false; return v.requestPictureInPicture(); } })()"#;

/// Seconds since 1970.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The tabs of one window, left to right.
#[derive(Default)]
struct WindowTabs {
    order: Vec<u32>,
    active: Option<u32>,
    /// The tab that links were last opened from, and the last tab opened from it,
    /// so several links opened in a row line up next to each other, like Chrome.
    last_opened: Option<(u32, u32)>,
    groups: Vec<TabGroup>,
}

#[derive(Default)]
struct TabList {
    windows: HashMap<String, WindowTabs>,
    info: HashMap<u32, TabInfo>,
    next_id: u32,
    next_group: u32,
    /// Recently closed tabs (address, title), newest last (for Ctrl+Shift+T).
    closed: Vec<(String, String)>,
}

impl TabList {
    fn pinned_count(&self, window: &str) -> usize {
        self.windows.get(window).map_or(0, |w| {
            w.order.iter().filter(|id| self.info.get(id).is_some_and(|t| t.pinned)).count()
        })
    }
}

/// Where a new tab goes in the tab row.
#[derive(Clone, Copy)]
pub enum Place {
    /// At the end (new tab button, Ctrl+T).
    End,
    /// Right after the given tab, after the tabs already opened from it (links).
    NextTo(u32),
}

/// All open tabs, shared between Raven's commands and the tab webviews.
#[derive(Default)]
pub struct Tabs(Mutex<TabList>);

#[derive(Clone, Serialize)]
pub struct TabsState {
    window: String,
    tabs: Vec<TabInfo>,
    active: Option<u32>,
    groups: Vec<TabGroup>,
    /// The tab shown at the right in split view.
    split: Option<u32>,
}

/// Script added to every website: middle click and Ctrl+click on a link ask
/// Raven to open it in a background tab, by "visiting" a raven-background: address
/// that Raven catches and cancels. Ctrl + mouse wheel asks for zoom the same way.
/// Websites get no other way to talk to Raven.
const LINK_SCRIPT: &str = r#"(() => {
  const BACKGROUND = "raven-background:";
  function linkOf(event) {
    const target = event.target instanceof Element ? event.target.closest("a[href]") : null;
    return target && /^https?:/i.test(target.href) ? target.href : null;
  }
  function openInBackground(event, url) {
    event.preventDefault();
    event.stopImmediatePropagation();
    location.href = BACKGROUND + url;
  }
  addEventListener("auxclick", (event) => {
    if (event.button !== 1) return;
    const url = linkOf(event);
    if (url) openInBackground(event, url);
  }, true);
  addEventListener("click", (event) => {
    if (event.button !== 0 || !(event.ctrlKey || event.metaKey) || event.shiftKey) return;
    const url = linkOf(event);
    if (url) openInBackground(event, url);
  }, true);
  // Ctrl + mouse wheel zooms the page, like other browsers.
  let lastZoom = 0;
  addEventListener("wheel", (event) => {
    if (!event.ctrlKey) return;
    event.preventDefault();
    const now = Date.now();
    if (now - lastZoom < 120) return;
    lastZoom = now;
    location.href = "raven-action:" + (event.deltaY < 0 ? "zoom-in" : "zoom-out");
  }, { capture: true, passive: false });
})();"#;
const BACKGROUND_PREFIX: &str = "raven-background:";
/// The only actions a website's page may ask for (Ctrl + mouse wheel).
const ACTION_PREFIX: &str = "raven-action:";

/// True for Raven's new tab page, whichever way the address is written:
/// tauri.localhost in the installed app, 127.0.0.1 while developing (npm run dev).
pub fn is_new_tab_url(url: &str) -> bool {
    if url == NEW_TAB || url.is_empty() {
        return true;
    }
    let Ok(parsed) = url.parse::<Url>() else { return false };
    let own_host = matches!(
        parsed.host_str(),
        Some("tauri.localhost" | "localhost" | "127.0.0.1")
    );
    own_host && parsed.path() == format!("/{NEW_TAB_PAGE}")
}

/// Websites get the usual white page background, Raven's new tab page its dark one.
fn background_for(url: &str) -> tauri::window::Color {
    if is_new_tab_url(url) {
        windows::BACKGROUND
    } else {
        WHITE
    }
}

fn label(id: u32) -> String {
    format!("tab-{id}")
}

/// Runs `f` with the tab list locked. Keep `f` short: never create or close webviews inside it.
fn with_tabs<T>(app: &AppHandle, f: impl FnOnce(&mut TabList) -> T) -> T {
    let state = app.state::<Tabs>();
    let mut list = state.0.lock().unwrap();
    f(&mut list)
}

pub fn window_of_tab(app: &AppHandle, id: u32) -> Option<String> {
    with_tabs(app, |list| list.info.get(&id).map(|tab| tab.window.clone()))
}

fn snapshot(app: &AppHandle, window: &str) -> TabsState {
    let split = windows::split_of(app, window);
    let mut state = with_tabs(app, |list| {
        let tabs = list.windows.get(window).map_or_else(Vec::new, |w| {
            w.order.iter().filter_map(|id| list.info.get(id).cloned()).collect()
        });
        let active = list.windows.get(window).and_then(|w| w.active);
        let groups = list.windows.get(window).map(|w| w.groups.clone()).unwrap_or_default();
        TabsState { window: window.to_string(), tabs, active, groups, split: None }
    });
    state.split = split;
    state
}

/// Saves the open windows and tabs (private windows are never saved).
fn save_session(app: &AppHandle) {
    let mut labels = windows::window_labels(app);
    labels.retain(|window| !windows::is_private(app, window));
    // Oldest window first: "win-1", "win-2", ...
    labels.sort_by_key(|window| window.trim_start_matches("win-").parse::<u32>().unwrap_or(0));
    let saved = with_tabs(app, |list| {
        labels
            .iter()
            .filter_map(|window| {
                let tabs = list.windows.get(window)?;
                let saved_tabs: Vec<session::SavedTab> = tabs
                    .order
                    .iter()
                    .filter_map(|id| list.info.get(id))
                    .map(|tab| session::SavedTab {
                        url: tab.url.clone(),
                        title: tab.title.clone(),
                        pinned: tab.pinned,
                        last_used: tab.last_used,
                        group: tab.group,
                    })
                    .collect();
                let active = tabs
                    .active
                    .and_then(|id| tabs.order.iter().position(|t| *t == id))
                    .unwrap_or(0);
                Some(session::SavedWindow { tabs: saved_tabs, active, groups: tabs.groups.clone() })
            })
            .filter(|window| !window.tabs.is_empty())
            .collect::<Vec<_>>()
    });
    session::save(app, session::Session { windows: saved, ..Default::default() });
}

/// Tells a window's toolbar what its tabs look like now, and saves the session.
fn publish(app: &AppHandle, window: &str) {
    let state = snapshot(app, window);
    if !windows::is_private(app, window) {
        save_session(app);
    }
    let _ = app.emit("tabs", state);
}

fn update_tab(app: &AppHandle, id: u32, change: impl FnOnce(&mut TabInfo)) {
    let window = with_tabs(app, |list| {
        let info = list.info.get_mut(&id)?;
        change(info);
        Some(info.window.clone())
    });
    if let Some(window) = window {
        publish(app, &window);
    }
}

/// Called from media.rs when a page starts or stops playing sound.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn set_audible(app: &AppHandle, id: u32, audible: bool) {
    update_tab(app, id, |tab| tab.audible = audible);
}

/// Turns what the user typed into a web address: a URL, a domain, or a search
/// with the chosen search engine (`search_url` ends where the words go).
pub fn to_url(input: &str, search_url: &str) -> Result<Url, String> {
    let text = input.trim();
    if text.is_empty() {
        return HOME_URL.parse().map_err(|e| format!("{e}"));
    }
    if text.starts_with("http://") || text.starts_with("https://") || text.starts_with("chrome-extension://") {
        return text.parse().map_err(|e| format!("{e}"));
    }
    if text.contains('.') && !text.contains(' ') {
        if let Ok(url) = format!("https://{text}").parse() {
            return Ok(url);
        }
    }
    let words: String = Url::parse_with_params("https://x/", &[("q", text)])
        .map_err(|e| format!("{e}"))?
        .query()
        .unwrap_or("")
        .trim_start_matches("q=")
        .to_string();
    format!("{search_url}{words}").parse().map_err(|e| format!("{e}"))
}

/// Adds a tab to a window's list, without a webview yet.
fn add_tab(app: &AppHandle, window: &str, url: &str, title: &str, place: Place, pinned: bool) -> u32 {
    let new_tab = is_new_tab_url(url);
    let private = windows::is_private(app, window);
    with_tabs(app, |list| {
        list.next_id += 1;
        let id = list.next_id;
        let pinned_count = list.pinned_count(window);
        // Links opened from a grouped tab join its group, like Chrome.
        let inherited = match place {
            Place::NextTo(opener) if !pinned => list.info.get(&opener).and_then(|t| t.group),
            _ => None,
        };
        let tabs = list.windows.entry(window.to_string()).or_default();
        let mut index = match place {
            Place::End => tabs.order.len(),
            Place::NextTo(opener) => match tabs.order.iter().position(|t| *t == opener) {
                Some(opener_index) => {
                    let after = match tabs.last_opened {
                        Some((from, child)) if from == opener => tabs
                            .order
                            .iter()
                            .position(|t| *t == child)
                            .unwrap_or(opener_index),
                        _ => opener_index,
                    };
                    tabs.last_opened = Some((opener, id));
                    after + 1
                }
                None => tabs.order.len(),
            },
        };
        // Pinned tabs come first, then the others.
        index = if pinned { index.min(pinned_count) } else { index.max(pinned_count) };
        tabs.order.insert(index.min(tabs.order.len()), id);
        list.info.insert(
            id,
            TabInfo {
                id,
                window: window.to_string(),
                title: if title.is_empty() { NEW_TAB_TITLE.into() } else { title.into() },
                url: if new_tab { String::new() } else { url.to_string() },
                loading: false,
                asleep: true,
                private,
                zoom: 1.0,
                pinned,
                audible: false,
                muted: false,
                memory: None,
                last_seen: std::time::Instant::now(),
                last_used: now_secs(),
                group: inherited,
            },
        );
        id
    })
}

/// Adds a restored tab that loads only when it is opened.
pub fn add_sleeping_tab(app: &AppHandle, window: &str, url: &str, title: &str, pinned: bool, last_used: u64) -> u32 {
    let id = add_tab(app, window, url, title, Place::End, pinned);
    if last_used > 0 {
        with_tabs(app, |list| {
            if let Some(info) = list.info.get_mut(&id) {
                info.last_used = last_used;
            }
        });
    }
    id
}

/// A tab as the performance watcher sees it.
#[allow(dead_code)]
pub struct TabUse {
    pub id: u32,
    pub window: String,
    pub url: String,
    pub title: String,
    pub active: bool,
    pub asleep: bool,
    pub pinned: bool,
    pub audible: bool,
    pub private: bool,
    pub idle: std::time::Duration,
    pub last_used: u64,
}

pub fn all_tabs(app: &AppHandle) -> Vec<TabUse> {
    with_tabs(app, |list| {
        list.info
            .values()
            .map(|tab| TabUse {
                id: tab.id,
                window: tab.window.clone(),
                url: tab.url.clone(),
                title: tab.title.clone(),
                active: list.windows.get(&tab.window).and_then(|w| w.active) == Some(tab.id),
                asleep: tab.asleep,
                pinned: tab.pinned,
                audible: tab.audible,
                private: tab.private,
                idle: tab.last_seen.elapsed(),
                last_used: tab.last_used,
            })
            .collect()
    })
}

/// Puts a tab to sleep: its page is closed to free memory and loads again
/// when the tab is opened. Returns false for tabs that must stay awake.
pub fn sleep_tab(app: &AppHandle, id: u32) -> bool {
    // The right-hand tab of split view is on screen.
    if window_of_tab(app, id).is_some_and(|w| windows::split_of(app, &w) == Some(id)) {
        return false;
    }
    let window = with_tabs(app, |list| {
        let info = list.info.get(&id)?;
        let active = list.windows.get(&info.window).and_then(|w| w.active) == Some(id);
        if active || info.asleep || info.audible {
            return None;
        }
        let window = info.window.clone();
        let info = list.info.get_mut(&id)?;
        info.asleep = true;
        info.loading = false;
        info.memory = None;
        Some(window)
    });
    let Some(window) = window else { return false };
    if let Some(webview) = app.get_webview(&label(id)) {
        let _ = webview.close();
    }
    publish(app, &window);
    true
}

/// Puts every tab of a window to sleep except the one you are looking at.
pub fn sleep_others(app: &AppHandle, window: &str) {
    for id in tab_ids(app, window) {
        sleep_tab(app, id);
    }
}

/// Saves the measured memory of tabs (MB) for their hover cards.
pub fn set_memory(app: &AppHandle, pages: &[(String, u64)]) {
    let changed: Vec<String> = with_tabs(app, |list| {
        let mut windows = Vec::new();
        for info in list.info.values_mut() {
            let mb = if info.asleep {
                None
            } else {
                pages.iter().find(|(url, _)| *url == info.url).map(|(_, bytes)| (*bytes / 1_048_576) as u32)
            };
            if info.memory != mb {
                info.memory = mb;
                if !windows.contains(&info.window) {
                    windows.push(info.window.clone());
                }
            }
        }
        windows
    });
    for window in changed {
        publish(app, &window);
    }
}

/// Creates the webview of a tab and loads its page.
fn wake(app: &AppHandle, id: u32) -> tauri::Result<Webview> {
    let (url, private, window_label) = with_tabs(app, |list| {
        let info = list.info.get_mut(&id)?;
        info.asleep = false;
        info.loading = true;
        Some((info.url.clone(), info.private, info.window.clone()))
    })
    .ok_or(tauri::Error::WebviewNotFound)?;

    let new_tab = is_new_tab_url(&url);
    let target = if new_tab {
        WebviewUrl::App(NEW_TAB_PAGE.into())
    } else {
        WebviewUrl::External(url.parse().map_err(|_| tauri::Error::InvalidWebviewUrl("bad url"))?)
    };

    let window = app.get_window(&window_label).ok_or(tauri::Error::WindowNotFound)?;
    let scale = window.scale_factor()?;
    let size = window.inner_size()?.to_logical::<f64>(scale);

    let on_load = app.clone();
    let on_title = app.clone();
    let on_popup = app.clone();
    let on_link = app.clone();
    let on_download = app.clone();
    let on_permission = app.clone();
    let builder = extensions::allow(WebviewBuilder::new(label(id), target))
        .background_color(background_for(&url))
        // Private windows use WebView2's InPrivate mode.
        .incognito(private)
        .initialization_script(LINK_SCRIPT)
        // Global Privacy Control, the script side of the Sec-GPC header.
        .initialization_script("Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', { get: () => true, configurable: true });")
        .initialization_script(extensions::STORE_SCRIPT)
        .on_navigation(move |url| {
            if let Some(action) = url.as_str().strip_prefix(ACTION_PREFIX) {
                // "Sor" on the new tab page: a question for the AI panel.
                if let Some(text) = action.strip_prefix("ask-ai:") {
                    let app = on_link.clone();
                    let question = percent_decode(text);
                    tauri::async_runtime::spawn(async move {
                        if let Some(window) = window_of_tab(&app, id) {
                            crate::ai::ask_direct(&app, &window, &question);
                        }
                    });
                    return false;
                }
                // "Raven'a ekle" on a Chrome Web Store page.
                if let Some(store_id) = action.strip_prefix("install-extension:") {
                    let app = on_link.clone();
                    let store_id = store_id.to_string();
                    tauri::async_runtime::spawn(async move {
                        let title = with_tabs(&app, |list| list.info.get(&id).map(|t| t.title.clone()));
                        if let (Some(tab), Some(window)) = (app.get_webview(&label(id)), window_of_tab(&app, id)) {
                            let title = title.unwrap_or_default();
                            extensions::request_from_store(&app, &tab, &window, &title, &store_id);
                        }
                    });
                    return false;
                }
                let direction = match action {
                    "zoom-in" => 1,
                    "zoom-out" => -1,
                    _ => return false,
                };
                let app = on_link.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = zoom_tab(&app, id, direction);
                });
                return false;
            }
            // Tracking parameters (utm_source, fbclid...) are removed before the page loads.
            if crate::shield::cleans_urls(&on_link) {
                if let Some(clean) = crate::shield::clean_url(url) {
                    let app = on_link.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(tab) = app.get_webview(&label(id)) {
                            let _ = tab.navigate(clean);
                        }
                    });
                    return false;
                }
            }
            // Middle click or Ctrl+click on a link: open it next to this tab, in the background.
            let Some(link) = url.as_str().strip_prefix(BACKGROUND_PREFIX) else {
                return true;
            };
            if link.starts_with("http://") || link.starts_with("https://") {
                let app = on_link.clone();
                let link = link.to_string();
                tauri::async_runtime::spawn(async move {
                    if let Some(window) = window_of_tab(&app, id) {
                        let _ = create_tab_at(&app, &window, &link, false, Place::NextTo(id));
                    }
                });
            }
            false
        })
        .on_page_load(move |webview, payload| {
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let mut url = payload.url().to_string();
            let mut level = 1.0;
            if loading {
                crate::shield::page_started(&on_load, id);
                let _ = webview.set_background_color(Some(background_for(&url)));
                // Each website keeps its own zoom.
                level = zoom::level_for(&on_load, &url);
                let _ = webview.set_zoom(level);
            }
            if is_new_tab_url(&url) {
                // The address bar stays empty on the new tab page, like other browsers.
                url = String::new();
                if !loading && private {
                    let _ = webview.eval("document.body.classList.add('private')");
                }
            } else if !loading {
                if let Some(script) = crate::shield::cosmetic_script(&on_load, &url) {
                    let _ = webview.eval(script);
                }
                offer_translation(&on_load, webview, id, &url);
            }
            if !is_new_tab_url(&url) && !loading && !private {
                // The title usually arrives before the page finishes loading.
                let title = with_tabs(&on_load, |list| {
                    list.info.get(&id).map(|tab| tab.title.clone()).unwrap_or_default()
                });
                history::record(&on_load, &url, &title);
            }
            update_tab(&on_load, id, |tab| {
                tab.url = url;
                tab.loading = loading;
                if loading {
                    tab.zoom = level;
                }
            });
        })
        .on_document_title_changed(move |webview, title| {
            if title.trim().is_empty() {
                return;
            }
            if !private {
                if let Ok(url) = webview.url() {
                    history::set_title(&on_title, url.as_str(), &title);
                }
            }
            update_tab(&on_title, id, |tab| tab.title = title);
        })
        .on_download(move |_webview, event| {
            match event {
                DownloadEvent::Requested { url, destination } => {
                    downloads::started(&on_download, url.as_str(), destination);
                }
                DownloadEvent::Finished { url, path, success } => {
                    downloads::finished(&on_download, url.as_str(), path, success);
                }
                _ => {}
            }
            true
        })
        // Camera, microphone, location, notifications: the site's saved choice.
        .on_permission_request(move |webview, kind| {
            permissions::decide(&on_permission, &webview, kind)
        })
        // Links that open a new window open in a new Raven tab next to this one.
        .on_new_window(move |url, _features| {
            let app = on_popup.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(window) = window_of_tab(&app, id) {
                    let _ = create_tab_at(&app, &window, url.as_str(), true, Place::NextTo(id));
                }
            });
            NewWindowResponse::Deny
        });

    let top = windows::toolbar_height();
    let webview = window.add_child(
        builder,
        LogicalPosition::new(0.0, top),
        LogicalSize::new(size.width, (size.height - top).max(0.0)),
    )?;
    crate::shortcuts::listen_in_tab(app, &webview, id);
    media::watch(app, &webview, id);
    {
        let settings = app.state::<crate::settings::SettingsState>().0.lock().unwrap().clone();
        crate::native::configure(&webview, settings.save_passwords, settings.autofill);
        crate::native::set_color_scheme(&webview, &settings.site_scheme);
    }
    crate::native::watch_fullscreen(app, &webview, page_fullscreen);
    crate::shield::attach(app, &webview, id);
    {
        // A missing page (404) or a site that cannot be reached: the toolbar
        // offers the copy in the Internet Archive.
        let app = app.clone();
        crate::native::watch_failures(&webview, move |url, problem| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(window) = window_of_tab(&app, id) {
                    let _ = app.emit("page-failed", serde_json::json!({ "window": window, "tab": id, "url": url, "problem": problem }));
                }
            });
        });
    }
    {
        let app = app.clone();
        crate::native::extend_context_menu(
            &webview,
            std::sync::Arc::new(move |choice| {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let Some(window) = window_of_tab(&app, id) else { return };
                    match choice {
                        crate::native::MenuChoice::OpenLink(url) => {
                            let _ = create_tab_at(&app, &window, &url, false, Place::NextTo(id));
                        }
                        crate::native::MenuChoice::Search(text) => {
                            if let Ok(url) = to_url(&text, &crate::settings::search_url(&app)) {
                                let _ = create_tab_at(&app, &window, url.as_str(), true, Place::NextTo(id));
                            }
                        }
                        crate::native::MenuChoice::AskAi(text) => {
                            crate::ai::ask_direct(&app, &window, &format!("Bunu açıkla:\n\n{text}"));
                        }
                        crate::native::MenuChoice::SearchImage(source) => {
                            if let Ok(url) = tauri::Url::parse_with_params("https://lens.google.com/uploadbyurl", &[("url", source)]) {
                                let _ = create_tab_at(&app, &window, url.as_str(), true, Place::NextTo(id));
                            }
                        }
                        crate::native::MenuChoice::Summarize => {
                            let _ = crate::ai::ask_in(&app, &window, "summarize", None).await;
                        }
                    }
                });
            }),
        );
    }
    windows::tab_created(app, &window_label);
    // Place it as the page card (margins, rounded corners, AI panel, panels open).
    windows::layout(app, &window_label);
    Ok(webview)
}

/// Opens a new tab at the end of a window. Must not be called from a webview event handler.
pub fn create_tab(app: &AppHandle, window: &str, url: &str, activate: bool) -> tauri::Result<u32> {
    create_tab_at(app, window, url, activate, Place::End)
}

/// Opens a new tab at a given place. Must not be called from a webview event handler.
pub fn create_tab_at(
    app: &AppHandle,
    window: &str,
    url: &str,
    activate: bool,
    place: Place,
) -> tauri::Result<u32> {
    let id = add_tab(app, window, url, "", place, false);
    if activate {
        // Show the new tab in the tab row at once; the page loads after.
        with_tabs(app, |list| {
            if let Some(tabs) = list.windows.get_mut(window) {
                tabs.active = Some(id);
            }
        });
        publish(app, window);
        show_only(app, id);
    } else {
        wake(app, id)?.hide()?;
        publish(app, window);
    }
    Ok(id)
}

/// Shows one tab (waking it if it is asleep) and hides the other tabs of its
/// window. The new tab is shown before the others are hidden, so nothing
/// flashes in between.
pub fn show_only(app: &AppHandle, id: u32) {
    let mut leaving: Option<u32> = None;
    // Choosing the right-hand tab of split view: the two swap sides.
    if let Some(window) = window_of_tab(app, id) {
        if windows::split_of(app, &window) == Some(id) {
            if let Some(previous) = active_id(app, &window) {
                windows::set_split(app, &window, Some(previous));
            }
        }
    }
    let Some((window, changed)) = with_tabs(app, |list| {
        let window = list.info.get(&id)?.window.clone();
        let tabs = list.windows.get_mut(&window)?;
        // Switching away ends a row of links opened from one tab, like Chrome.
        if matches!(tabs.last_opened, Some((from, child)) if from != id && child != id) {
            tabs.last_opened = None;
        }
        let changed = tabs.active != Some(id);
        let previous = tabs.active.replace(id);
        leaving = previous.filter(|p| *p != id);
        // The tab you leave starts its time in the background now.
        for seen in [previous, Some(id)].into_iter().flatten() {
            if let Some(info) = list.info.get_mut(&seen) {
                info.last_seen = std::time::Instant::now();
            }
        }
        if let Some(info) = list.info.get_mut(&id) {
            info.last_seen = std::time::Instant::now();
            info.last_used = now_secs();
        }
        Some((window, changed))
    }) else {
        return;
    };
    // Mark the tab active in the tab row before the page is shown.
    if changed {
        publish(app, &window);
    }
    // A picture of the tab you leave, for its hover card.
    if let Some(old) = leaving.filter(|_| !windows::is_private(app, &window)) {
        if let Some(webview) = app.get_webview(&label(old)) {
            crate::previews::capture(app, &webview, old);
        }
    }
    let target = match app.get_webview(&label(id)) {
        Some(webview) => Some(webview),
        None => wake(app, id).map_err(|e| eprintln!("Could not open tab: {e}")).ok(),
    };
    if !windows::is_full_window(app, &window) {
        if let Some(webview) = target {
            let _ = webview.show();
        }
    }
    let partner = windows::split_of(app, &window).filter(|p| *p != id);
    for other in tab_ids(app, &window) {
        if other != id && Some(other) != partner {
            if let Some(webview) = app.get_webview(&label(other)) {
                let _ = webview.hide();
            }
        }
    }
    if partner.is_some() {
        windows::layout(app, &window);
    }
    publish(app, &window);
}

/// Moves a tab to a new window of its own (tab menu: "Yeni pencereye taşı").
pub fn move_to_new_window(app: &AppHandle, id: u32) -> Result<(), String> {
    let from = window_of_tab(app, id).ok_or("Tab not found")?;
    if tab_ids(app, &from).len() < 2 {
        return Err("Bu pencerede tek sekme var".into());
    }
    if windows::split_of(app, &from) == Some(id) {
        windows::set_split(app, &from, None);
    }
    let private = windows::is_private(app, &from);
    let to = windows::open_window(app, private, None).map_err(|e| e.to_string())?;
    let placeholder = active_id(app, &to);
    // Take the tab out of the old window, like closing it there.
    let next = with_tabs(app, |list| {
        let tabs = list.windows.get_mut(&from)?;
        let index = tabs.order.iter().position(|t| *t == id)?;
        tabs.order.remove(index);
        let next = if tabs.active == Some(id) {
            tabs.active = None;
            tabs.order.get(index).or_else(|| tabs.order.last()).copied()
        } else {
            None
        };
        let info = list.info.get_mut(&id)?;
        info.window = to.clone();
        info.group = None;
        info.pinned = false;
        list.windows.get_mut(&to)?.order.push(id);
        drop_empty_groups(list, &from);
        Some(next)
    })
    .ok_or("Tab not found")?;
    if let (Some(webview), Some(window)) = (app.get_webview(&label(id)), app.get_window(&to)) {
        webview.reparent(&window).map_err(|e| e.to_string())?;
    }
    show_only(app, id);
    if let Some(placeholder) = placeholder {
        let _ = close(app, placeholder);
    }
    match next {
        Some(next) => show_only(app, next),
        None => publish(app, &from),
    }
    windows::layout(app, &from);
    windows::layout(app, &to);
    Ok(())
}

/// Shows `id` at the right of the active tab (split view), or ends split view.
pub fn split_with(app: &AppHandle, id: u32, on: bool) -> Result<(), String> {
    let window = window_of_tab(app, id).ok_or("Tab not found")?;
    if !on {
        windows::set_split(app, &window, None);
        if let Some(active) = active_id(app, &window) {
            show_only(app, active);
        }
        return Ok(());
    }
    if active_id(app, &window) == Some(id) {
        return Err("Bu sekme zaten açık".into());
    }
    let webview = match app.get_webview(&label(id)) {
        Some(webview) => webview,
        None => wake(app, id).map_err(|e| e.to_string())?,
    };
    windows::set_split(app, &window, Some(id));
    let _ = webview.show();
    publish(app, &window);
    Ok(())
}

fn tab_ids(app: &AppHandle, window: &str) -> Vec<u32> {
    with_tabs(app, |list| list.windows.get(window).map(|w| w.order.clone()).unwrap_or_default())
}

/// The awake tab webviews of a window.
pub fn webviews_of(app: &AppHandle, window: &str) -> Vec<Webview> {
    tab_ids(app, window).into_iter().filter_map(|id| app.get_webview(&label(id))).collect()
}

/// Hides every tab of a window, while a full-window panel is open.
pub fn hide_all(app: &AppHandle, window: &str) {
    for webview in webviews_of(app, window) {
        let _ = webview.hide();
    }
}

/// Shows a window's active tab again.
pub fn show_active(app: &AppHandle, window: &str) {
    if let Some(id) = active_id(app, window) {
        show_only(app, id);
    }
}

/// Reloads every open new tab page, for example after the search engine changed.
pub fn reload_new_tab_pages(app: &AppHandle) {
    let ids: Vec<u32> = with_tabs(app, |list| {
        list.info.values().filter(|tab| tab.url.is_empty()).map(|tab| tab.id).collect()
    });
    for id in ids {
        if let Some(webview) = app.get_webview(&label(id)) {
            let _ = webview.eval("location.reload()");
        }
    }
}

pub fn active_tab_id(app: &AppHandle, window: &str) -> Option<u32> {
    active_id(app, window)
}

fn active_id(app: &AppHandle, window: &str) -> Option<u32> {
    with_tabs(app, |list| list.windows.get(window).and_then(|w| w.active))
}

fn active_tab(app: &AppHandle, window: &str) -> Result<Webview, String> {
    active_id(app, window)
        .and_then(|id| app.get_webview(&label(id)))
        .ok_or_else(|| "No open tab".to_string())
}

/// The address and title of a window's active tab (empty address on the new tab page).
pub fn active_page(app: &AppHandle, window: &str) -> Option<(String, String)> {
    with_tabs(app, |list| {
        let id = list.windows.get(window)?.active?;
        let info = list.info.get(&id)?;
        Some((info.url.clone(), info.title.clone()))
    })
}

/// The webview of a window's active tab, if it is awake.
pub fn active_webview(app: &AppHandle, window: &str) -> Option<Webview> {
    let id = active_id(app, window)?;
    app.get_webview(&label(id))
}

/// A page went in or out of full screen (a video's full screen button).
fn page_fullscreen(app: &AppHandle, webview: &Webview, on: bool) {
    let window = crate::windows::window_of(webview);
    crate::windows::set_fullscreen(app, &window, on);
}

/// Applies changed settings to the open tabs (passwords, forms, site colours).
pub fn apply_settings(app: &AppHandle) {
    let settings = app.state::<crate::settings::SettingsState>().0.lock().unwrap().clone();
    for (label, webview) in app.webviews() {
        if label.starts_with("tab-") {
            crate::native::configure(&webview, settings.save_passwords, settings.autofill);
            crate::native::set_color_scheme(&webview, &settings.site_scheme);
        }
    }
}

/// Opens a tab in a window and shows it.
pub fn open_tab(app: &AppHandle, window: &str, url: &str) -> Result<u32, String> {
    create_tab(app, window, url, true).map_err(|e| e.to_string())
}

/// Loads an address in a window's active tab.
pub fn open_here(app: &AppHandle, window: &str, url: &str) -> Result<(), String> {
    let url: Url = url.parse().map_err(|e| format!("{e}"))?;
    active_tab(app, window)?.navigate(url).map_err(|e| e.to_string())
}

/// Closes a tab. Closing the last tab of a window leaves a new tab page.
pub fn close(app: &AppHandle, id: u32) -> Result<(), String> {
    // The window's last tab: instead of closing the window, go to a new tab
    // page. If it already is one, nothing happens.
    let last = with_tabs(app, |list| {
        let info = list.info.get(&id)?;
        let count = list.windows.get(&info.window)?.order.len();
        (count == 1).then(|| (info.window.clone(), is_new_tab_url(&info.url) || info.url.is_empty()))
    });
    if let Some((window, new_tab_page)) = last {
        if new_tab_page {
            return Ok(());
        }
        // The new tab opens first, so closing this one leaves it.
        create_tab(app, &window, NEW_TAB, true).map_err(|e| e.to_string())?;
    }

    let result = with_tabs(app, |list| {
        let info = list.info.remove(&id)?;
        if !info.private {
            list.closed.push((info.url.clone(), info.title.clone()));
            if list.closed.len() > MAX_CLOSED {
                list.closed.remove(0);
            }
        }
        let tabs = list.windows.get_mut(&info.window)?;
        let index = tabs.order.iter().position(|t| *t == id)?;
        tabs.order.remove(index);
        let next = if tabs.active == Some(id) {
            tabs.active = None;
            // Go to the tab on the right, or the one on the left if it was the last.
            tabs.order.get(index).or_else(|| tabs.order.last()).copied()
        } else {
            tabs.active
        };
        Some((info.window, next))
    });
    // The tab was already closed.
    let Some((window, next)) = result else {
        return Ok(());
    };
    crate::previews::forget(app, id);
    // Split view ends when one of its two tabs closes.
    if let Some(split) = windows::split_of(app, &window) {
        if split == id || next != active_id(app, &window) || Some(split) == next {
            windows::set_split(app, &window, None);
        }
    }

    match next {
        // Show the next tab first, then close this one, so nothing flashes in between.
        Some(next) => {
            show_only(app, next);
            if let Some(webview) = app.get_webview(&label(id)) {
                webview.close().map_err(|e| e.to_string())?;
            }
        }
        None => {
            if let Some(handle) = app.get_window(&window) {
                handle.close().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn close_active(app: &AppHandle, window: &str) -> Result<(), String> {
    match active_id(app, window) {
        Some(id) => close(app, id),
        None => Ok(()),
    }
}

/// A page in another language: the toolbar offers to translate it (Chrome does
/// the same). The page's language comes from its <html lang>.
fn offer_translation(app: &AppHandle, webview: Webview, id: u32, url: &str) {
    if !app.state::<settings::SettingsState>().0.lock().unwrap().offer_translate {
        return;
    }
    let host = tauri::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
    if host.is_empty() || host.contains("translate.goog") || host.contains("translate.google") {
        return;
    }
    let app = app.clone();
    let url = url.to_string();
    std::thread::spawn(move || {
        let script = r#"(document.documentElement.lang || document.querySelector("meta[http-equiv='content-language' i]")?.content || "").trim().slice(0, 2).toLowerCase()"#;
        let Ok(json) = crate::native::run_script(&webview, script) else { return };
        let lang: String = serde_json::from_str(&json).unwrap_or_default();
        if lang.len() != 2 || lang == "tr" {
            return;
        }
        if let Some(window) = window_of_tab(&app, id) {
            let _ = app.emit("offer-translate", serde_json::json!({ "window": window, "tab": id, "url": url, "host": host, "lang": lang }));
        }
    });
}

/// Opens the most recently closed tab again.
pub fn reopen_closed(app: &AppHandle, window: &str) -> Result<(), String> {
    match with_tabs(app, |list| list.closed.pop()) {
        Some((url, _)) => open_tab(app, window, if url.is_empty() { NEW_TAB } else { &url }).map(|_| ()),
        None => Ok(()),
    }
}

#[derive(Serialize)]
pub struct ClosedTab {
    url: String,
    title: String,
}

/// Recently closed tabs, newest first (the history page shows them).
#[tauri::command]
pub async fn get_closed_tabs(app: AppHandle) -> Vec<ClosedTab> {
    with_tabs(&app, |list| {
        list.closed
            .iter()
            .rev()
            .filter(|(url, _)| !url.is_empty())
            .map(|(url, title)| ClosedTab { url: url.clone(), title: title.clone() })
            .collect()
    })
}

/// Opens a recently closed tab again and takes it off the list.
#[tauri::command]
pub async fn reopen_closed_tab(app: AppHandle, webview: Webview, url: String) -> Result<(), String> {
    with_tabs(&app, |list| {
        if let Some(index) = list.closed.iter().rposition(|(u, _)| *u == url) {
            list.closed.remove(index);
        }
    });
    open_tab(&app, &windows::window_of(&webview), &url).map(|_| ())
}

/// Moves to the next (+1) or previous (-1) tab of a window, wrapping around.
pub fn switch_relative(app: &AppHandle, window: &str, step: isize) -> Result<(), String> {
    let target = with_tabs(app, |list| {
        let tabs = list.windows.get(window)?;
        let count = tabs.order.len() as isize;
        if count == 0 {
            return None;
        }
        let current = tabs
            .active
            .and_then(|id| tabs.order.iter().position(|t| *t == id))
            .unwrap_or(0) as isize;
        let index = (current + step).rem_euclid(count) as usize;
        Some(tabs.order[index])
    });
    if let Some(id) = target {
        show_only(app, id);
    }
    Ok(())
}

pub fn switch_to_index(app: &AppHandle, window: &str, index: usize) -> Result<(), String> {
    if let Some(id) = tab_ids(app, window).get(index).copied() {
        show_only(app, id);
    }
    Ok(())
}

pub fn switch_to_last(app: &AppHandle, window: &str) -> Result<(), String> {
    if let Some(id) = tab_ids(app, window).last().copied() {
        show_only(app, id);
    }
    Ok(())
}

/// Zooms a tab in (+1), out (-1) or back to 100% (0). The new level is remembered
/// for the website and applied to its other open tabs too, like Chrome.
pub fn zoom_tab(app: &AppHandle, id: u32, direction: i32) -> Result<(), String> {
    let (url, current) = with_tabs(app, |list| {
        let info = list.info.get(&id)?;
        Some((info.url.clone(), info.zoom))
    })
    .ok_or("Tab not found")?;
    let Some(site) = zoom::site_of(&url) else { return Ok(()) };
    let level = zoom::step(current, direction);
    zoom::remember(app, &url, level);
    let changed: Vec<(u32, String)> = with_tabs(app, |list| {
        list.info
            .values_mut()
            .filter(|tab| zoom::site_of(&tab.url).as_deref() == Some(site.as_str()))
            .map(|tab| {
                tab.zoom = level;
                (tab.id, tab.window.clone())
            })
            .collect()
    });
    let mut windows_changed: Vec<String> = Vec::new();
    for (tab, window) in changed {
        if let Some(webview) = app.get_webview(&label(tab)) {
            let _ = webview.set_zoom(level);
        }
        if !windows_changed.contains(&window) {
            windows_changed.push(window);
        }
    }
    for window in windows_changed {
        publish(app, &window);
    }
    Ok(())
}

pub fn zoom_active(app: &AppHandle, window: &str, direction: i32) -> Result<(), String> {
    match active_id(app, window) {
        Some(id) => zoom_tab(app, id, direction),
        None => Ok(()),
    }
}

/// Pins a tab (it moves to the end of the pinned tabs) or unpins it (it moves
/// to the start of the other tabs), like Chrome.
pub fn set_pinned(app: &AppHandle, id: u32, pinned: bool) -> Result<(), String> {
    let window = with_tabs(app, |list| {
        let window = list.info.get(&id)?.window.clone();
        list.info.get_mut(&id)?.pinned = false;
        let pinned_count = list.pinned_count(&window);
        let tabs = list.windows.get_mut(&window)?;
        tabs.order.retain(|t| *t != id);
        tabs.order.insert(pinned_count.min(tabs.order.len()), id);
        list.info.get_mut(&id)?.pinned = pinned;
        Some(window)
    })
    .ok_or("Tab not found")?;
    publish(app, &window);
    Ok(())
}

/// Moves a tab to another place in its window's tab row (drag and drop).
/// Pinned tabs stay among the pinned ones, others among the others.
pub fn move_tab(app: &AppHandle, id: u32, index: usize) -> Result<(), String> {
    let window = with_tabs(app, |list| {
        let info = list.info.get(&id)?;
        let (window, pinned) = (info.window.clone(), info.pinned);
        let pinned_count = list.pinned_count(&window);
        let tabs = list.windows.get_mut(&window)?;
        let from = tabs.order.iter().position(|t| *t == id)?;
        tabs.order.remove(from);
        let (low, high) = if pinned {
            (0, pinned_count.saturating_sub(1))
        } else {
            (pinned_count, tabs.order.len())
        };
        let to = index.clamp(low, high.max(low)).min(tabs.order.len());
        tabs.order.insert(to, id);
        // Dropped inside a group: it joins; dragged out: it leaves.
        if !pinned {
            let map = group_map(list, &window);
            let order = list.windows.get(&window)?.order.clone();
            let group = group_after_move(&order, &map, id);
            list.info.get_mut(&id)?.group = group;
            drop_empty_groups(list, &window);
        }
        Some(window)
    })
    .ok_or("Tab not found")?;
    publish(app, &window);
    Ok(())
}

// Tab groups.

/// After `id` moved, it joins the group it landed inside, or leaves its group
/// if it is no longer next to it. Pure, for tests.
pub fn group_after_move(order: &[u32], group_of: &HashMap<u32, Option<u32>>, id: u32) -> Option<u32> {
    let at = order.iter().position(|t| *t == id)?;
    let left = at.checked_sub(1).and_then(|i| order.get(i)).and_then(|t| group_of.get(t).copied().flatten());
    let right = order.get(at + 1).and_then(|t| group_of.get(t).copied().flatten());
    let own = group_of.get(&id).copied().flatten();
    match (left, right) {
        (Some(l), Some(r)) if l == r => Some(l),
        _ if own.is_some() && (left == own || right == own) => own,
        _ => None,
    }
}

/// Where a tab goes when it joins group `group`: right after the group's last tab.
pub fn group_insert_index(order: &[u32], group_of: &HashMap<u32, Option<u32>>, group: u32) -> Option<usize> {
    order.iter().rposition(|t| group_of.get(t).copied().flatten() == Some(group)).map(|i| i + 1)
}

fn group_map(list: &TabList, window: &str) -> HashMap<u32, Option<u32>> {
    list.windows
        .get(window)
        .map(|w| w.order.iter().map(|id| (*id, list.info.get(id).and_then(|t| t.group))).collect())
        .unwrap_or_default()
}

/// Drops groups that have no tabs left.
fn drop_empty_groups(list: &mut TabList, window: &str) {
    let used: std::collections::HashSet<u32> = list
        .windows
        .get(window)
        .map(|w| w.order.iter().filter_map(|id| list.info.get(id).and_then(|t| t.group)).collect())
        .unwrap_or_default();
    if let Some(tabs) = list.windows.get_mut(window) {
        tabs.groups.retain(|g| used.contains(&g.id));
    }
}

/// Puts a tab in a group (a new one when `group` is None), or takes it out (`join` false).
pub fn set_group(app: &AppHandle, id: u32, group: Option<u32>, join: bool) -> Result<(), String> {
    let window = with_tabs(app, |list| {
        let info = list.info.get(&id)?;
        if info.pinned {
            return None;
        }
        let window = info.window.clone();
        let old = info.group;
        let target = if !join {
            None
        } else if let Some(group) = group {
            Some(group)
        } else {
            list.next_group += 1;
            let new_id = list.next_group;
            let used = list.windows.get(&window).map(|w| w.groups.len()).unwrap_or(0);
            let color = GROUP_COLORS[used % GROUP_COLORS.len()].to_string();
            list.windows.get_mut(&window)?.groups.push(TabGroup { id: new_id, name: String::new(), color, collapsed: false });
            Some(new_id)
        };
        list.info.get_mut(&id)?.group = target;
        // Keep groups together: move the tab next to its group, or out of its old one.
        let map = group_map(list, &window);
        let tabs = list.windows.get_mut(&window)?;
        let place = match (target, old) {
            (Some(g), _) => {
                let mut without = tabs.order.clone();
                without.retain(|t| *t != id);
                group_insert_index(&without, &map, g)
            }
            (None, Some(g)) => {
                let mut without = tabs.order.clone();
                without.retain(|t| *t != id);
                group_insert_index(&without, &map, g)
            }
            _ => None,
        };
        if let Some(index) = place {
            tabs.order.retain(|t| *t != id);
            let index = index.min(tabs.order.len());
            tabs.order.insert(index, id);
        }
        drop_empty_groups(list, &window);
        Some(window)
    })
    .ok_or("Sabit sekmeler gruplanamaz")?;
    publish(app, &window);
    Ok(())
}

/// Renames, recolours, folds or unfolds a group.
pub fn update_group(app: &AppHandle, window: &str, group: u32, name: Option<String>, color: Option<String>, collapsed: Option<bool>) {
    let switch_away = with_tabs(app, |list| {
        let tabs = list.windows.get_mut(window)?;
        let g = tabs.groups.iter_mut().find(|g| g.id == group)?;
        if let Some(name) = name {
            g.name = name.chars().take(40).collect();
        }
        if let Some(color) = color.filter(|c| GROUP_COLORS.contains(&c.as_str())) {
            g.color = color;
        }
        if let Some(collapsed) = collapsed {
            g.collapsed = collapsed;
        }
        // Folding the group of the open tab: open the nearest tab outside it.
        let active = tabs.active?;
        if !collapsed.unwrap_or(false) || list.info.get(&active)?.group != Some(group) {
            return None;
        }
        let at = tabs.order.iter().position(|t| *t == active)?;
        let outside = |t: &u32| list.info.get(t).is_some_and(|i| i.group != Some(group));
        tabs.order[at..].iter().find(|t| outside(t)).or_else(|| tabs.order[..at].iter().rev().find(|t| outside(t))).copied()
    });
    match switch_away {
        Some(other) => show_only(app, other),
        None => publish(app, window),
    }
}

/// Takes every tab out of a group.
pub fn ungroup(app: &AppHandle, window: &str, group: u32) {
    with_tabs(app, |list| {
        for info in list.info.values_mut() {
            if info.window == window && info.group == Some(group) {
                info.group = None;
            }
        }
        drop_empty_groups(list, window);
    });
    publish(app, window);
}

/// Closes every tab of a group.
pub fn close_group(app: &AppHandle, window: &str, group: u32) {
    let ids: Vec<u32> = with_tabs(app, |list| {
        list.windows
            .get(window)
            .map(|w| w.order.iter().filter(|id| list.info.get(id).is_some_and(|t| t.group == Some(group))).copied().collect())
            .unwrap_or_default()
    });
    for id in ids {
        let _ = close(app, id);
    }
}

/// Restores a window's groups from the session.
pub fn restore_groups(app: &AppHandle, window: &str, groups: &[TabGroup], members: &[(u32, u32)]) {
    with_tabs(app, |list| {
        let next = groups.iter().map(|g| g.id).max().unwrap_or(0);
        list.next_group = list.next_group.max(next);
        if let Some(tabs) = list.windows.get_mut(window) {
            tabs.groups = groups.to_vec();
        }
        for (tab, group) in members {
            if let Some(info) = list.info.get_mut(tab) {
                info.group = Some(*group);
            }
        }
        drop_empty_groups(list, window);
    });
}

#[tauri::command]
pub async fn group_command(
    app: AppHandle,
    webview: Webview,
    group: u32,
    action: String,
    name: Option<String>,
    color: Option<String>,
) -> Result<(), String> {
    let window = windows::window_of(&webview);
    match action.as_str() {
        "rename" => update_group(&app, &window, group, name, None, None),
        "color" => update_group(&app, &window, group, None, color, None),
        "fold" => update_group(&app, &window, group, None, None, Some(true)),
        "unfold" => update_group(&app, &window, group, None, None, Some(false)),
        "ungroup" => ungroup(&app, &window, group),
        "close" => close_group(&app, &window, group),
        "new-tab" => {
            let id = create_tab(&app, &window, NEW_TAB, true).map_err(|e| e.to_string())?;
            update_group(&app, &window, group, None, None, Some(false));
            set_group(&app, id, Some(group), true)?;
        }
        _ => return Err(format!("Unknown group action {action}")),
    }
    Ok(())
}

/// Mutes or unmutes a tab's sound.
pub fn set_muted(app: &AppHandle, id: u32, muted: bool) -> Result<(), String> {
    if let Some(webview) = app.get_webview(&label(id)) {
        media::set_muted(&webview, muted);
    }
    update_tab(app, id, |tab| tab.muted = muted);
    Ok(())
}

/// Opens the same page again in a new tab next to it.
pub fn duplicate(app: &AppHandle, id: u32) -> Result<(), String> {
    let (window, url) = with_tabs(app, |list| {
        let info = list.info.get(&id)?;
        Some((info.window.clone(), info.url.clone()))
    })
    .ok_or("Tab not found")?;
    let url = if url.is_empty() { NEW_TAB.to_string() } else { url };
    create_tab_at(app, &window, &url, true, Place::NextTo(id)).map(|_| ()).map_err(|e| e.to_string())
}

/// Closes the other tabs of the window (pinned tabs stay), or only those to the right.
pub fn close_others(app: &AppHandle, id: u32, only_right: bool) -> Result<(), String> {
    let doomed: Vec<u32> = with_tabs(app, |list| {
        let window = list.info.get(&id)?.window.clone();
        let order = list.windows.get(&window)?.order.clone();
        let position = order.iter().position(|t| *t == id)?;
        Some(
            order
                .iter()
                .enumerate()
                .filter(|(index, other)| {
                    **other != id
                        && (!only_right || *index > position)
                        && !list.info.get(other).is_some_and(|t| t.pinned)
                })
                .map(|(_, other)| *other)
                .collect(),
        )
    })
    .unwrap_or_default();
    show_only(app, id);
    for other in doomed {
        close(app, other)?;
    }
    Ok(())
}

pub fn reload_tab(app: &AppHandle, id: u32) -> Result<(), String> {
    match app.get_webview(&label(id)) {
        Some(webview) => webview.eval("location.reload()").map_err(|e| e.to_string()),
        // An asleep tab loads fresh when opened anyway.
        None => Ok(()),
    }
}

/// A window was closed: forget its tabs. When other windows are still open,
/// the session no longer includes it; when it was the last, the session keeps
/// it so the next start brings it back.
pub fn forget_window(app: &AppHandle, window: &str, others_left: bool) {
    with_tabs(app, |list| {
        if let Some(tabs) = list.windows.remove(window) {
            for id in tabs.order {
                list.info.remove(&id);
            }
        }
    });
    if others_left {
        save_session(app);
    }
}

// Commands called by a window's toolbar or popup. They are async so that Windows
// can create and close webviews without blocking the window. `webview` is the
// caller, which tells which window it is about.

#[tauri::command]
pub async fn get_tabs(app: AppHandle, webview: Webview) -> TabsState {
    snapshot(&app, &windows::window_of(&webview))
}

#[tauri::command]
pub async fn switch_tab(app: AppHandle, id: u32) -> Result<(), String> {
    if !with_tabs(&app, |list| list.info.contains_key(&id)) {
        return Err("Tab not found".into());
    }
    show_only(&app, id);
    Ok(())
}

#[tauri::command]
pub async fn close_tab(app: AppHandle, id: u32) -> Result<(), String> {
    close(&app, id)
}

#[tauri::command]
pub async fn move_tab_command(app: AppHandle, id: u32, index: usize) -> Result<(), String> {
    move_tab(&app, id, index)
}

/// The tab menu (right click on a tab) and the sound button on tabs.
#[tauri::command]
pub async fn tab_action(app: AppHandle, id: u32, action: String) -> Result<(), String> {
    match action.as_str() {
        "new-tab-right" => {
            let window = window_of_tab(&app, id).ok_or("Tab not found")?;
            create_tab_at(&app, &window, NEW_TAB, true, Place::NextTo(id))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "reload" => reload_tab(&app, id),
        "duplicate" => duplicate(&app, id),
        "pin" => set_pinned(&app, id, true),
        "unpin" => set_pinned(&app, id, false),
        "mute" => set_muted(&app, id, true),
        "unmute" => set_muted(&app, id, false),
        "close" => close(&app, id),
        "close-others" => close_others(&app, id, false),
        "close-right" => close_others(&app, id, true),
        "split" => split_with(&app, id, true),
        "new-window" => move_to_new_window(&app, id),
        "unsplit" => split_with(&app, id, false),
        "group-new" => set_group(&app, id, None, true),
        "ungroup" => set_group(&app, id, None, false),
        other if other.starts_with("group-") => {
            let group = other.trim_start_matches("group-").parse().map_err(|_| "Bad group")?;
            set_group(&app, id, Some(group), true)
        }
        "reopen-closed" => {
            let window = window_of_tab(&app, id).ok_or("Tab not found")?;
            reopen_closed(&app, &window)
        }
        _ => Err(format!("Unknown tab action {action}")),
    }
}

/// Pauses or plays the videos and sounds of a tab (media button in the toolbar).
#[tauri::command]
pub async fn media_command(app: AppHandle, id: u32, action: String) -> Result<(), String> {
    let tab = app.get_webview(&label(id)).ok_or("Tab is asleep")?;
    let script = match action.as_str() {
        "pause" => r#"(() => { window.__ravenPaused = [...document.querySelectorAll("video, audio")].filter((m) => !m.paused); window.__ravenPaused.forEach((m) => m.pause()); })()"#,
        // A floating video window that stays on top of everything.
        "pip" => {
            crate::native::run_as_user(&tab, PIP_SCRIPT);
            return Ok(());
        }
        "play" => r#"(() => { (window.__ravenPaused || [...document.querySelectorAll("video, audio")].slice(0, 1)).forEach((m) => m.play().catch(() => {})); window.__ravenPaused = null; })()"#,
        _ => return Err("Unknown media action".into()),
    };
    tab.eval(script).map_err(|e| e.to_string())
}

/// Opens an address from a bookmark, the history or a suggestion, here or in a new tab.
#[tauri::command]
pub async fn open_url(app: AppHandle, webview: Webview, url: String, new_tab: bool) -> Result<(), String> {
    let window = windows::window_of(&webview);
    if new_tab {
        open_tab(&app, &window, &url).map(|_| ())
    } else {
        open_here(&app, &window, &url)
    }
}

#[tauri::command]
pub async fn navigate(app: AppHandle, webview: Webview, input: String) -> Result<(), String> {
    let url = to_url(&input, &settings::search_url(&app))?;
    let url = if app.state::<settings::SettingsState>().0.lock().unwrap().https_upgrade { upgrade(url) } else { url };
    active_tab(&app, &windows::window_of(&webview))?.navigate(url).map_err(|e| e.to_string())
}

/// What was typed in the address bar, opened in a new tab (Alt+Enter).
#[tauri::command]
pub async fn navigate_new_tab(app: AppHandle, webview: Webview, input: String) -> Result<(), String> {
    let url = to_url(&input, &settings::search_url(&app))?;
    let url = if app.state::<settings::SettingsState>().0.lock().unwrap().https_upgrade { upgrade(url) } else { url };
    open_tab(&app, &windows::window_of(&webview), url.as_str()).map(|_| ())
}

#[tauri::command]
pub async fn go_back(app: AppHandle, webview: Webview) -> Result<(), String> {
    active_tab(&app, &windows::window_of(&webview))?
        .eval("history.back()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn go_forward(app: AppHandle, webview: Webview) -> Result<(), String> {
    active_tab(&app, &windows::window_of(&webview))?
        .eval("history.forward()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reload(app: AppHandle, webview: Webview) -> Result<(), String> {
    active_tab(&app, &windows::window_of(&webview))?
        .eval("location.reload()")
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::is_new_tab_url;

    const GOOGLE: &str = "https://www.google.com/search?q=";

    fn to_url(input: &str) -> Result<tauri::Url, String> {
        super::to_url(input, GOOGLE)
    }

    #[test]
    fn groups_follow_moves() {
        use std::collections::HashMap;
        let map: HashMap<u32, Option<u32>> = [(1, None), (2, Some(7)), (3, Some(7)), (4, None), (5, Some(7))].into();
        // 5 dropped between 2 and 3: stays in 7.
        assert_eq!(super::group_after_move(&[1, 2, 5, 3, 4], &map, 5), Some(7));
        // 1 dropped between 2 and 3: joins 7.
        assert_eq!(super::group_after_move(&[2, 1, 3, 4], &map, 1), Some(7));
        // 3 dragged to the far right: leaves 7.
        assert_eq!(super::group_after_move(&[1, 2, 4, 3], &map, 3), None);
        // 3 at the group's edge stays.
        assert_eq!(super::group_after_move(&[1, 2, 3, 4], &map, 3), Some(7));
        assert_eq!(super::group_insert_index(&[1, 2, 3, 4], &map, 7), Some(3));
        assert_eq!(super::group_insert_index(&[1, 4], &map, 7), None);
    }

    #[test]
    fn upgrades_to_https() {
        let up = |u: &str| super::upgrade(u.parse().unwrap()).to_string();
        assert_eq!(up("http://example.com/a"), "https://example.com/a");
        assert_eq!(up("http://localhost:3000/"), "http://localhost:3000/");
        assert_eq!(up("http://192.168.1.1/"), "http://192.168.1.1/");
        assert_eq!(up("http://router/"), "http://router/");
        assert_eq!(up("https://a.com/"), "https://a.com/");
    }

    #[test]
    fn decodes_actions() {
        assert_eq!(super::percent_decode("Merhaba%20d%C3%BCnya%3F"), "Merhaba dünya?");
        assert_eq!(super::percent_decode("50%"), "50%");
    }

    #[test]
    fn keeps_full_urls() {
        assert_eq!(to_url("https://github.com").unwrap().as_str(), "https://github.com/");
    }

    #[test]
    fn adds_https_to_domains() {
        assert_eq!(to_url("f2pclub.app").unwrap().as_str(), "https://f2pclub.app/");
    }

    #[test]
    fn searches_plain_text() {
        let url = to_url("raven browser").unwrap();
        assert_eq!(url.host_str(), Some("www.google.com"));
        assert_eq!(url.query(), Some("q=raven+browser"));
    }

    #[test]
    fn uses_the_chosen_search_engine() {
        let url = super::to_url("çay & simit", "https://duckduckgo.com/?q=").unwrap();
        assert_eq!(url.host_str(), Some("duckduckgo.com"));
        assert_eq!(url.query(), Some("q=%C3%A7ay+%26+simit"));
    }

    #[test]
    fn empty_goes_home() {
        assert_eq!(to_url("  ").unwrap().as_str(), "https://www.google.com/");
    }

    #[test]
    fn recognizes_the_new_tab_page() {
        assert!(is_new_tab_url("raven:newtab"));
        assert!(is_new_tab_url("http://tauri.localhost/newtab.html"));
        assert!(is_new_tab_url("tauri://localhost/newtab.html"));
        assert!(is_new_tab_url("http://127.0.0.1:1430/newtab.html"));
        assert!(!is_new_tab_url("https://example.com/newtab.html"));
        assert!(!is_new_tab_url("https://www.google.com/"));
    }
}
