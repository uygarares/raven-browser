//! Tabs: each tab is its own webview inside the main window.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl,
};

use crate::TOOLBAR_HEIGHT;

pub const HOME_URL: &str = "https://www.google.com";
/// Opens Raven's own new tab page (ui/newtab.html).
pub const NEW_TAB: &str = "raven:newtab";
const NEW_TAB_PAGE: &str = "newtab.html";
const WHITE: tauri::window::Color = tauri::window::Color(255, 255, 255, 255);
const SEARCH_URL: &str = "https://www.google.com/search";
const NEW_TAB_TITLE: &str = "Yeni sekme";

#[derive(Clone, Serialize)]
pub struct TabInfo {
    id: u32,
    title: String,
    url: String,
    loading: bool,
}

#[derive(Default)]
struct TabList {
    order: Vec<u32>,
    info: HashMap<u32, TabInfo>,
    active: Option<u32>,
    next_id: u32,
    /// Addresses of recently closed tabs, newest last (for Ctrl+Shift+T).
    closed: Vec<String>,
}

const MAX_CLOSED: usize = 25;

/// All open tabs, shared between Raven's commands and the tab webviews.
#[derive(Default)]
pub struct Tabs(Mutex<TabList>);

#[derive(Clone, Serialize)]
pub struct TabsState {
    tabs: Vec<TabInfo>,
    active: Option<u32>,
}

/// True for Raven's new tab page, whichever way the address is written.
pub fn is_new_tab_url(url: &str) -> bool {
    url == NEW_TAB
        || url.is_empty()
        || ((url.starts_with("tauri://localhost/")
            || url.starts_with("http://tauri.localhost/")
            || url.starts_with("https://tauri.localhost/"))
            && url.contains(NEW_TAB_PAGE))
}

/// Websites get the usual white page background. Raven's new tab page is
/// see-through so the glass theme can show the desktop.
fn background_for(url: &str) -> tauri::window::Color {
    if is_new_tab_url(url) {
        crate::CLEAR
    } else {
        WHITE
    }
}

fn label(id: u32) -> String {
    format!("tab-{id}")
}

pub fn is_tab_label(label: &str) -> bool {
    label.starts_with("tab-")
}

/// Runs `f` with the tab list locked. Keep `f` short: never create or close webviews inside it.
fn with_tabs<T>(app: &AppHandle, f: impl FnOnce(&mut TabList) -> T) -> T {
    let state = app.state::<Tabs>();
    let mut list = state.0.lock().unwrap();
    f(&mut list)
}

fn snapshot(app: &AppHandle) -> TabsState {
    with_tabs(app, |list| TabsState {
        tabs: list.order.iter().filter_map(|id| list.info.get(id).cloned()).collect(),
        active: list.active,
    })
}

/// Tells the toolbar what the tabs look like now.
fn publish(app: &AppHandle) {
    let _ = app.emit("tabs", snapshot(app));
}

fn update_tab(app: &AppHandle, id: u32, change: impl FnOnce(&mut TabInfo)) {
    let found = with_tabs(app, |list| match list.info.get_mut(&id) {
        Some(info) => {
            change(info);
            true
        }
        None => false,
    });
    if found {
        publish(app);
    }
}

/// Turns what the user typed into a web address: a URL, a domain, or a search.
pub fn to_url(input: &str) -> Result<Url, String> {
    let text = input.trim();
    if text.is_empty() {
        return HOME_URL.parse().map_err(|e| format!("{e}"));
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return text.parse().map_err(|e| format!("{e}"));
    }
    if text.contains('.') && !text.contains(' ') {
        if let Ok(url) = format!("https://{text}").parse() {
            return Ok(url);
        }
    }
    Url::parse_with_params(SEARCH_URL, &[("q", text)]).map_err(|e| format!("{e}"))
}

/// Opens a new tab. Must not be called from a webview event handler (use a spawned task).
pub fn create_tab(app: &AppHandle, url: &str, activate: bool) -> tauri::Result<u32> {
    let new_tab = is_new_tab_url(url);
    let target = if new_tab {
        WebviewUrl::App(NEW_TAB_PAGE.into())
    } else {
        WebviewUrl::External(url.parse().map_err(|_| tauri::Error::InvalidWebviewUrl("bad url"))?)
    };
    let shown_url = if new_tab { String::new() } else { url.to_string() };
    let id = with_tabs(app, |list| {
        list.next_id += 1;
        let id = list.next_id;
        list.order.push(id);
        list.info.insert(
            id,
            TabInfo { id, title: NEW_TAB_TITLE.into(), url: shown_url, loading: true },
        );
        id
    });

    let window = app.get_window("main").ok_or(tauri::Error::WindowNotFound)?;
    let scale = window.scale_factor()?;
    let size = window.inner_size()?.to_logical::<f64>(scale);

    let on_load = app.clone();
    let on_title = app.clone();
    let on_popup = app.clone();
    let builder = WebviewBuilder::new(label(id), target)
        .transparent(true)
        .background_color(background_for(if new_tab { NEW_TAB } else { url }))
        .on_page_load(move |webview, payload| {
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let mut url = payload.url().to_string();
            if loading {
                let _ = webview.set_background_color(Some(background_for(&url)));
            }
            if is_new_tab_url(&url) {
                // The address bar stays empty on the new tab page, like other browsers.
                url = String::new();
            }
            update_tab(&on_load, id, |tab| {
                tab.url = url;
                tab.loading = loading;
            });
        })
        .on_document_title_changed(move |_webview, title| {
            update_tab(&on_title, id, |tab| {
                if !title.trim().is_empty() {
                    tab.title = title;
                }
            });
        })
        // Links that open a new window open in a new Raven tab instead.
        .on_new_window(move |url, _features| {
            let app = on_popup.clone();
            tauri::async_runtime::spawn(async move {
                let _ = create_tab(&app, url.as_str(), true);
            });
            NewWindowResponse::Deny
        });

    let webview = window.add_child(
        builder,
        LogicalPosition::new(0.0, TOOLBAR_HEIGHT),
        LogicalSize::new(size.width, (size.height - TOOLBAR_HEIGHT).max(0.0)),
    )?;
    crate::shortcuts::listen_in_tab(app, &webview);

    if activate {
        show_only(app, id);
    } else {
        let _ = webview.hide();
        publish(app);
    }
    Ok(id)
}

/// Shows one tab and hides the others. The new tab is shown before the others
/// are hidden, so nothing flashes in between.
fn show_only(app: &AppHandle, id: u32) {
    with_tabs(app, |list| list.active = Some(id));
    let target = label(id);
    let webviews = app.webviews();
    if !crate::is_full_window() {
        if let Some(webview) = webviews.get(&target) {
            let _ = webview.show();
        }
    }
    for (name, webview) in webviews {
        if is_tab_label(&name) && name != target {
            let _ = webview.hide();
        }
    }
    publish(app);
}

/// Hides every tab, while a full-window screen is open.
pub fn hide_all(app: &AppHandle) {
    for (name, webview) in app.webviews() {
        if is_tab_label(&name) {
            let _ = webview.hide();
        }
    }
}

/// Shows the active tab again.
pub fn show_active(app: &AppHandle) {
    if let Some(id) = with_tabs(app, |list| list.active) {
        show_only(app, id);
    }
}

/// Reloads every open new tab page, for example after the theme changed.
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

fn active_tab(app: &AppHandle) -> Result<Webview, String> {
    let active = with_tabs(app, |list| list.active);
    active
        .and_then(|id| app.get_webview(&label(id)))
        .ok_or_else(|| "No open tab".to_string())
}

/// Opens a tab and shows it.
pub fn open_tab(app: &AppHandle, url: &str) -> Result<u32, String> {
    create_tab(app, url, true).map_err(|e| e.to_string())
}

/// Closes a tab. Closing the last tab closes Raven, like other browsers.
pub fn close(app: &AppHandle, id: u32) -> Result<(), String> {
    let next = with_tabs(app, |list| {
        let index = list.order.iter().position(|t| *t == id)?;
        list.order.remove(index);
        if let Some(info) = list.info.remove(&id) {
            list.closed.push(info.url);
            if list.closed.len() > MAX_CLOSED {
                list.closed.remove(0);
            }
        }
        if list.active == Some(id) {
            list.active = None;
            // Go to the tab on the right, or the one on the left if it was the last.
            Some(list.order.get(index).or_else(|| list.order.last()).copied())
        } else {
            Some(list.active)
        }
    });
    // The tab was already closed.
    let Some(next) = next else {
        return Ok(());
    };

    match next {
        // Show the next tab first, then close this one, so nothing flashes in between.
        Some(next) => {
            show_only(app, next);
            if let Some(webview) = app.get_webview(&label(id)) {
                webview.close().map_err(|e| e.to_string())?;
            }
        }
        None => {
            if let Some(window) = app.get_window("main") {
                window.close().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn close_active(app: &AppHandle) -> Result<(), String> {
    match with_tabs(app, |list| list.active) {
        Some(id) => close(app, id),
        None => Ok(()),
    }
}

/// Opens the most recently closed tab again.
pub fn reopen_closed(app: &AppHandle) -> Result<(), String> {
    match with_tabs(app, |list| list.closed.pop()) {
        Some(url) => open_tab(app, if url.is_empty() { NEW_TAB } else { &url }).map(|_| ()),
        None => Ok(()),
    }
}

/// Moves to the next (+1) or previous (-1) tab, wrapping around.
pub fn switch_relative(app: &AppHandle, step: isize) -> Result<(), String> {
    let target = with_tabs(app, |list| {
        let count = list.order.len() as isize;
        if count == 0 {
            return None;
        }
        let current = list
            .active
            .and_then(|id| list.order.iter().position(|t| *t == id))
            .unwrap_or(0) as isize;
        let index = (current + step).rem_euclid(count) as usize;
        Some(list.order[index])
    });
    if let Some(id) = target {
        show_only(app, id);
    }
    Ok(())
}

pub fn switch_to_index(app: &AppHandle, index: usize) -> Result<(), String> {
    if let Some(id) = with_tabs(app, |list| list.order.get(index).copied()) {
        show_only(app, id);
    }
    Ok(())
}

pub fn switch_to_last(app: &AppHandle) -> Result<(), String> {
    if let Some(id) = with_tabs(app, |list| list.order.last().copied()) {
        show_only(app, id);
    }
    Ok(())
}

// Commands called by the toolbar. They are async so that Windows can create
// and close webviews without blocking the window.

#[tauri::command]
pub async fn get_tabs(app: AppHandle) -> TabsState {
    snapshot(&app)
}

#[tauri::command]
pub async fn new_tab(app: AppHandle) -> Result<u32, String> {
    open_tab(&app, NEW_TAB)
}

#[tauri::command]
pub async fn switch_tab(app: AppHandle, id: u32) -> Result<(), String> {
    if app.get_webview(&label(id)).is_none() {
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
pub async fn navigate(app: AppHandle, input: String) -> Result<(), String> {
    let url = to_url(&input)?;
    active_tab(&app)?.navigate(url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn go_back(app: AppHandle) -> Result<(), String> {
    active_tab(&app)?.eval("history.back()").map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn go_forward(app: AppHandle) -> Result<(), String> {
    active_tab(&app)?.eval("history.forward()").map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reload(app: AppHandle) -> Result<(), String> {
    active_tab(&app)?.eval("location.reload()").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::to_url;

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
    fn empty_goes_home() {
        assert_eq!(to_url("  ").unwrap().as_str(), "https://www.google.com/");
    }
}

#[cfg(test)]
mod new_tab_tests {
    use super::is_new_tab_url;

    #[test]
    fn recognizes_the_new_tab_page() {
        assert!(is_new_tab_url("raven:newtab"));
        assert!(is_new_tab_url("http://tauri.localhost/newtab.html"));
        assert!(is_new_tab_url("tauri://localhost/newtab.html"));
        assert!(!is_new_tab_url("https://example.com/newtab.html"));
        assert!(!is_new_tab_url("https://www.google.com/"));
    }
}
