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
}

/// All open tabs, shared between Raven's commands and the tab webviews.
#[derive(Default)]
pub struct Tabs(Mutex<TabList>);

#[derive(Clone, Serialize)]
pub struct TabsState {
    tabs: Vec<TabInfo>,
    active: Option<u32>,
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
    let url: Url = url.parse().map_err(|_| tauri::Error::InvalidWebviewUrl("bad url"))?;
    let id = with_tabs(app, |list| {
        list.next_id += 1;
        let id = list.next_id;
        list.order.push(id);
        list.info.insert(
            id,
            TabInfo { id, title: NEW_TAB_TITLE.into(), url: url.to_string(), loading: true },
        );
        id
    });

    let window = app.get_window("main").ok_or(tauri::Error::WindowNotFound)?;
    let scale = window.scale_factor()?;
    let size = window.inner_size()?.to_logical::<f64>(scale);

    let on_load = app.clone();
    let on_title = app.clone();
    let on_popup = app.clone();
    let builder = WebviewBuilder::new(label(id), WebviewUrl::External(url))
        .on_page_load(move |_webview, payload| {
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let url = payload.url().to_string();
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

    if activate {
        show_only(app, id);
    } else {
        let _ = webview.hide();
        publish(app);
    }
    Ok(id)
}

/// Shows one tab and hides the others.
fn show_only(app: &AppHandle, id: u32) {
    with_tabs(app, |list| list.active = Some(id));
    let target = label(id);
    for (name, webview) in app.webviews() {
        if !is_tab_label(&name) {
            continue;
        }
        if name == target {
            let _ = webview.show();
        } else {
            let _ = webview.hide();
        }
    }
    publish(app);
}

fn active_tab(app: &AppHandle) -> Result<Webview, String> {
    let active = with_tabs(app, |list| list.active);
    active
        .and_then(|id| app.get_webview(&label(id)))
        .ok_or_else(|| "No open tab".to_string())
}

// Commands called by the toolbar. They are async so that Windows can create
// and close webviews without blocking the window.

#[tauri::command]
pub async fn get_tabs(app: AppHandle) -> TabsState {
    snapshot(&app)
}

#[tauri::command]
pub async fn new_tab(app: AppHandle) -> Result<u32, String> {
    create_tab(&app, HOME_URL, true).map_err(|e| e.to_string())
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
    let next = with_tabs(&app, |list| {
        let index = list.order.iter().position(|t| *t == id)?;
        list.order.remove(index);
        list.info.remove(&id);
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

    if let Some(webview) = app.get_webview(&label(id)) {
        webview.close().map_err(|e| e.to_string())?;
    }

    match next {
        Some(next) => show_only(&app, next),
        // Closing the last tab closes Raven, like other browsers.
        None => {
            if let Some(window) = app.get_window("main") {
                window.close().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
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
