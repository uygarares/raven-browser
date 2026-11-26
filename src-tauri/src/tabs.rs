//! Tabs: each awake tab is its own webview inside the main window.
//!
//! A tab can also be asleep: it is listed in the tab row, but has no webview and
//! uses no memory until you click it. Tabs restored from the last session start
//! asleep (except the active one).

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl,
};

use crate::{downloads, history, session, settings, toolbar_height, zoom};
use tauri::webview::DownloadEvent;

pub const HOME_URL: &str = "https://www.google.com";
/// Opens Raven's own new tab page (ui/newtab.html).
pub const NEW_TAB: &str = "raven:newtab";
const NEW_TAB_PAGE: &str = "newtab.html";
const WHITE: tauri::window::Color = tauri::window::Color(255, 255, 255, 255);
const NEW_TAB_TITLE: &str = "Yeni sekme";

#[derive(Clone, Serialize)]
pub struct TabInfo {
    id: u32,
    title: String,
    /// Empty for Raven's new tab page.
    url: String,
    loading: bool,
    /// No webview yet: the page loads when the tab is opened.
    asleep: bool,
    /// Private tab: not saved in history or the session; its cookies and
    /// site data are deleted when the last private tab closes.
    private: bool,
    /// Page zoom, 1.0 = 100%.
    zoom: f64,
}

#[derive(Default)]
struct TabList {
    order: Vec<u32>,
    info: HashMap<u32, TabInfo>,
    active: Option<u32>,
    next_id: u32,
    /// Addresses of recently closed tabs, newest last (for Ctrl+Shift+T).
    closed: Vec<String>,
    /// The tab that links were last opened from, and the last tab opened from it,
    /// so several links opened in a row line up next to each other, like Chrome.
    last_opened: Option<(u32, u32)>,
}

/// Where a new tab goes in the tab row.
#[derive(Clone, Copy)]
pub enum Place {
    /// At the end (new tab button, Ctrl+T).
    End,
    /// Right after the given tab, after the tabs already opened from it (links).
    NextTo(u32),
}

/// Script added to every website: middle click and Ctrl+click on a link ask
/// Raven to open it in a background tab, by "visiting" a raven-background: address
/// that Raven catches and cancels. Websites get no other way to talk to Raven.
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

const MAX_CLOSED: usize = 25;

/// All open tabs, shared between Raven's commands and the tab webviews.
#[derive(Default)]
pub struct Tabs(Mutex<TabList>);

#[derive(Clone, Serialize)]
pub struct TabsState {
    tabs: Vec<TabInfo>,
    active: Option<u32>,
}

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
        crate::BACKGROUND
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

/// Tells the toolbar what the tabs look like now, and saves the session.
fn publish(app: &AppHandle) {
    let state = snapshot(app);
    let active = state
        .active
        .and_then(|id| state.tabs.iter().position(|tab| tab.id == id))
        .unwrap_or(0);
    // Private tabs are never saved.
    let saved: Vec<&TabInfo> = state.tabs.iter().filter(|tab| !tab.private).collect();
    let active = state
        .tabs
        .get(active)
        .and_then(|tab| saved.iter().position(|saved| saved.id == tab.id))
        .unwrap_or(0);
    session::save(
        app,
        session::Session {
            tabs: saved
                .iter()
                .map(|tab| session::SavedTab { url: tab.url.clone(), title: tab.title.clone() })
                .collect(),
            active,
        },
    );
    let _ = app.emit("tabs", state);
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

/// Turns what the user typed into a web address: a URL, a domain, or a search
/// with the chosen search engine (`search_url` ends where the words go).
pub fn to_url(input: &str, search_url: &str) -> Result<Url, String> {
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
    let words: String = Url::parse_with_params("https://x/", &[("q", text)])
        .map_err(|e| format!("{e}"))?
        .query()
        .unwrap_or("")
        .trim_start_matches("q=")
        .to_string();
    format!("{search_url}{words}").parse().map_err(|e| format!("{e}"))
}

/// Adds a tab to the list, without a webview yet.
fn add_tab(app: &AppHandle, url: &str, title: &str, place: Place, private: bool) -> u32 {
    let new_tab = is_new_tab_url(url);
    with_tabs(app, |list| {
        list.next_id += 1;
        let id = list.next_id;
        let index = match place {
            Place::End => list.order.len(),
            Place::NextTo(opener) => match list.order.iter().position(|t| *t == opener) {
                Some(opener_index) => {
                    let after = match list.last_opened {
                        Some((from, child)) if from == opener => list
                            .order
                            .iter()
                            .position(|t| *t == child)
                            .unwrap_or(opener_index),
                        _ => opener_index,
                    };
                    list.last_opened = Some((opener, id));
                    after + 1
                }
                None => list.order.len(),
            },
        };
        list.order.insert(index, id);
        list.info.insert(
            id,
            TabInfo {
                id,
                title: if title.is_empty() { NEW_TAB_TITLE.into() } else { title.into() },
                url: if new_tab { String::new() } else { url.to_string() },
                loading: false,
                asleep: true,
                private,
                zoom: 1.0,
            },
        );
        id
    })
}

/// Adds a restored tab that loads only when it is opened.
pub fn add_sleeping_tab(app: &AppHandle, url: &str, title: &str) -> u32 {
    add_tab(app, url, title, Place::End, false)
}

/// Creates the webview of a tab and loads its page.
fn wake(app: &AppHandle, id: u32) -> tauri::Result<Webview> {
    let (url, private) = with_tabs(app, |list| {
        let info = list.info.get_mut(&id)?;
        info.asleep = false;
        info.loading = true;
        Some((info.url.clone(), info.private))
    })
    .ok_or(tauri::Error::WebviewNotFound)?;

    let new_tab = is_new_tab_url(&url);
    let target = if new_tab {
        WebviewUrl::App(NEW_TAB_PAGE.into())
    } else {
        WebviewUrl::External(url.parse().map_err(|_| tauri::Error::InvalidWebviewUrl("bad url"))?)
    };

    let window = app.get_window("main").ok_or(tauri::Error::WindowNotFound)?;
    let scale = window.scale_factor()?;
    let size = window.inner_size()?.to_logical::<f64>(scale);

    let on_load = app.clone();
    let on_title = app.clone();
    let on_popup = app.clone();
    let on_link = app.clone();
    let on_download = app.clone();
    let builder = WebviewBuilder::new(label(id), target)
        .background_color(background_for(&url))
        // Private tabs use WebView2's InPrivate mode.
        .incognito(private)
        .initialization_script(LINK_SCRIPT)
        // Middle click or Ctrl+click on a link: open it next to this tab, in the background.
        .on_navigation(move |url| {
            if let Some(action) = url.as_str().strip_prefix(ACTION_PREFIX) {
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
            let Some(link) = url.as_str().strip_prefix(BACKGROUND_PREFIX) else {
                return true;
            };
            if link.starts_with("http://") || link.starts_with("https://") {
                let app = on_link.clone();
                let link = link.to_string();
                tauri::async_runtime::spawn(async move {
                    let _ = create_tab_at(&app, &link, false, Place::NextTo(id), private);
                });
            }
            false
        })
        .on_page_load(move |webview, payload| {
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let mut url = payload.url().to_string();
            let mut level = 1.0;
            if loading {
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
            } else if !loading && !private {
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
        // Links that open a new window open in a new Raven tab next to this one.
        .on_new_window(move |url, _features| {
            let app = on_popup.clone();
            tauri::async_runtime::spawn(async move {
                let _ = create_tab_at(&app, url.as_str(), true, Place::NextTo(id), private);
            });
            NewWindowResponse::Deny
        });

    let top = toolbar_height();
    let webview = window.add_child(
        builder,
        LogicalPosition::new(0.0, top),
        LogicalSize::new(size.width, (size.height - top).max(0.0)),
    )?;
    crate::shortcuts::listen_in_tab(app, &webview);
    crate::tab_created();
    if crate::is_full_window() {
        crate::layout(app);
    }
    Ok(webview)
}

/// Opens a new tab. Must not be called from a webview event handler (use a spawned task).
pub fn create_tab(app: &AppHandle, url: &str, activate: bool) -> tauri::Result<u32> {
    create_tab_at(app, url, activate, Place::End, false)
}

/// Opens a new tab at a given place. Must not be called from a webview event handler.
pub fn create_tab_at(
    app: &AppHandle,
    url: &str,
    activate: bool,
    place: Place,
    private: bool,
) -> tauri::Result<u32> {
    let id = add_tab(app, url, "", place, private);
    if activate {
        show_only(app, id);
    } else {
        wake(app, id)?.hide()?;
        publish(app);
    }
    Ok(id)
}

/// Shows one tab (waking it if it is asleep) and hides the others. The new tab is
/// shown before the others are hidden, so nothing flashes in between.
pub fn show_only(app: &AppHandle, id: u32) {
    with_tabs(app, |list| {
        // Switching away ends a row of links opened from one tab, like Chrome.
        if matches!(list.last_opened, Some((from, child)) if from != id && child != id) {
            list.last_opened = None;
        }
        list.active = Some(id);
    });
    let target = label(id);
    let target_webview = match app.get_webview(&target) {
        Some(webview) => Some(webview),
        None => wake(app, id).map_err(|e| eprintln!("Could not open tab: {e}")).ok(),
    };
    if !crate::is_full_window() {
        if let Some(webview) = target_webview {
            let _ = webview.show();
        }
    }
    for (name, webview) in app.webviews() {
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

/// The address and title of the active tab (empty address on the new tab page).
pub fn active_page(app: &AppHandle) -> Option<(String, String)> {
    with_tabs(app, |list| {
        let id = list.active?;
        let info = list.info.get(&id)?;
        Some((info.url.clone(), info.title.clone()))
    })
}

/// Opens a tab and shows it.
pub fn open_tab(app: &AppHandle, url: &str) -> Result<u32, String> {
    create_tab(app, url, true).map_err(|e| e.to_string())
}

/// Opens a private tab with the new tab page (Ctrl+Shift+N).
pub fn open_private_tab(app: &AppHandle) -> Result<u32, String> {
    create_tab_at(app, NEW_TAB, true, Place::End, true).map_err(|e| e.to_string())
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
    let same_site: Vec<u32> = with_tabs(app, |list| {
        list.info
            .values_mut()
            .filter(|tab| zoom::site_of(&tab.url).as_deref() == Some(site.as_str()))
            .map(|tab| {
                tab.zoom = level;
                tab.id
            })
            .collect()
    });
    for tab in same_site {
        if let Some(webview) = app.get_webview(&label(tab)) {
            let _ = webview.set_zoom(level);
        }
    }
    publish(app);
    Ok(())
}

pub fn zoom_active(app: &AppHandle, direction: i32) -> Result<(), String> {
    match with_tabs(app, |list| list.active) {
        Some(id) => zoom_tab(app, id, direction),
        None => Ok(()),
    }
}

/// Loads an address in the active tab.
pub fn open_here(app: &AppHandle, url: &str) -> Result<(), String> {
    let url: Url = url.parse().map_err(|e| format!("{e}"))?;
    active_tab(app)?.navigate(url).map_err(|e| e.to_string())
}

/// Closes a tab. Closing the last tab closes Raven, like other browsers.
pub fn close(app: &AppHandle, id: u32) -> Result<(), String> {
    let next = with_tabs(app, |list| {
        let index = list.order.iter().position(|t| *t == id)?;
        list.order.remove(index);
        if let Some(info) = list.info.remove(&id).filter(|info| !info.private) {
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

/// Opens an address from a bookmark or the history, here or in a new tab.
#[tauri::command]
pub async fn open_url(app: AppHandle, url: String, new_tab: bool) -> Result<(), String> {
    if new_tab {
        open_tab(&app, &url).map(|_| ())
    } else {
        open_here(&app, &url)
    }
}

#[tauri::command]
pub async fn navigate(app: AppHandle, input: String) -> Result<(), String> {
    let url = to_url(&input, &settings::search_url(&app))?;
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
    use super::is_new_tab_url;

    const GOOGLE: &str = "https://www.google.com/search?q=";

    fn to_url(input: &str) -> Result<tauri::Url, String> {
        super::to_url(input, GOOGLE)
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
