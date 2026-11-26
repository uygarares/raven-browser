//! Raven's windows. Each window has:
//! - "ui-<window>": its toolbar (tabs, address bar, bookmarks bar, panels), from `ui/`
//! - "popup-<window>": one small webview for menus and boxes that open over the
//!   page (⋮ menu, zoom box, tab menu, site info, address suggestions)
//! - its own tabs ("tab-N" webviews, see tabs.rs)
//!
//! A private window opens every tab in WebView2's InPrivate mode.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    webview::WebviewBuilder, window::WindowBuilder, AppHandle, Emitter, LogicalPosition,
    LogicalSize, Manager, Webview, WebviewUrl, WindowEvent,
};

use crate::{extensions, session, tabs};

/// Height of the tab row and address bar, in logical pixels.
/// Must match `--toolbar-height` in ui/style.css.
pub const TOOLBAR_BASE: f64 = 84.0;
/// Extra height while the bookmarks bar is shown. Must match `--bookmarks-bar-height`.
const BOOKMARKS_BAR: f64 = 32.0;

/// Raven's dark background (the Solid theme's page color). Shown for a moment
/// before a page draws itself, instead of a white flash.
pub const BACKGROUND: tauri::window::Color = tauri::window::Color(17, 16, 32, 255);
/// The popup menus' color (--menu in ui/theme.css).
const MENU_BACKGROUND: tauri::window::Color = tauri::window::Color(25, 24, 47, 255);

static SHOW_BOOKMARKS_BAR: AtomicBool = AtomicBool::new(false);
static VERTICAL_TABS: AtomicBool = AtomicBool::new(false);
/// With vertical tabs only the address row is on top.
const TOOLBAR_VERTICAL: f64 = 44.0;
/// Width of the tab list at the left (ui/style.css --sidebar-width).
pub const SIDEBAR_WIDTH: f64 = 240.0;
static NEXT_WINDOW: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Default)]
struct WindowState {
    private: bool,
    /// The toolbar fills the whole window (history, downloads, settings).
    full_window: bool,
    /// The AI panel is open at the right.
    ai_panel: bool,
    /// F11 or a page in full screen (a video): no toolbar, no margins.
    fullscreen: bool,
    /// Split view: this tab shows at the right of the active one.
    split: Option<u32>,
}

/// The two halves of the page card in split view (8 px apart).
pub fn split_halves(page: Rect) -> (Rect, Rect) {
    let gap = PAGE_MARGIN;
    let half = ((page.width - gap) / 2.0).max(0.0);
    (
        Rect { width: half, ..page },
        Rect { x: page.x + half + gap, width: (page.width - half - gap).max(0.0), ..page },
    )
}

pub fn split_of(app: &AppHandle, window: &str) -> Option<u32> {
    with_windows(app, |states| states.get(window).and_then(|s| s.split))
}

/// Starts or ends split view in a window.
pub fn set_split(app: &AppHandle, window: &str, tab: Option<u32>) {
    with_windows(app, |states| {
        if let Some(state) = states.get_mut(window) {
            state.split = tab;
        }
    });
    layout(app, window);
}

/// Space around the page card, and its corner radius (logical pixels).
pub const PAGE_MARGIN: f64 = 8.0;
pub const PAGE_RADIUS: f64 = 12.0;
/// Width of the AI panel.
pub const AI_PANEL_WIDTH: f64 = 420.0;

/// Where the page and the AI panel go in a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The page card and the AI panel for a window of `size`, with a toolbar of
/// height `top`.
pub fn page_layout(size: (f64, f64), top: f64, left: f64, ai_panel: bool, fullscreen: bool) -> (Rect, Option<Rect>) {
    let (width, height) = size;
    if fullscreen {
        return (Rect { x: 0.0, y: 0.0, width, height }, None);
    }
    let m = PAGE_MARGIN;
    let inner_height = (height - top - m).max(0.0);
    if ai_panel && width > left + AI_PANEL_WIDTH + 320.0 {
        let page_width = (width - left - 2.0 * m - AI_PANEL_WIDTH).max(0.0);
        let panel = Rect { x: width - m - AI_PANEL_WIDTH, y: top, width: AI_PANEL_WIDTH, height: inner_height };
        (Rect { x: left, y: top, width: page_width, height: inner_height }, Some(panel))
    } else {
        (Rect { x: left, y: top, width: (width - left - m).max(0.0), height: inner_height }, None)
    }
}

#[derive(Default)]
pub struct Windows {
    states: Mutex<HashMap<String, WindowState>>,
    /// Windows whose popup was created before one of their tabs: webviews made
    /// later are drawn on top, so the popup must be made again before showing.
    popup_behind: Mutex<HashSet<String>>,
    /// What each window's popup shows now, for a popup page that just loaded.
    last_popup: Mutex<HashMap<String, PopupRequest>>,
}

fn with_windows<T>(app: &AppHandle, f: impl FnOnce(&mut HashMap<String, WindowState>) -> T) -> T {
    let state = app.state::<Windows>();
    let mut states = state.states.lock().unwrap();
    f(&mut states)
}

pub fn ui_label(window: &str) -> String {
    format!("ui-{window}")
}

pub fn popup_label(window: &str) -> String {
    format!("popup-{window}")
}

/// The window a webview (toolbar, popup or tab) belongs to.
pub fn window_of(webview: &Webview) -> String {
    webview.window().label().to_string()
}

pub fn is_private(app: &AppHandle, window: &str) -> bool {
    with_windows(app, |states| states.get(window).map(|s| s.private).unwrap_or(false))
}

pub fn is_full_window(app: &AppHandle, window: &str) -> bool {
    with_windows(app, |states| states.get(window).map(|s| s.full_window).unwrap_or(false))
}

pub fn window_labels(app: &AppHandle) -> Vec<String> {
    with_windows(app, |states| states.keys().cloned().collect())
}

/// Where the website area starts.
pub fn toolbar_height() -> f64 {
    let base = if VERTICAL_TABS.load(Ordering::SeqCst) { TOOLBAR_VERTICAL } else { TOOLBAR_BASE };
    if SHOW_BOOKMARKS_BAR.load(Ordering::SeqCst) {
        base + BOOKMARKS_BAR
    } else {
        base
    }
}

/// Space at the left of the page: the tab list with vertical tabs.
pub fn left_space() -> f64 {
    if VERTICAL_TABS.load(Ordering::SeqCst) { SIDEBAR_WIDTH } else { PAGE_MARGIN }
}

/// Tabs in a list at the left (like Edge and Brave) instead of a row on top.
pub fn set_vertical_tabs(app: &AppHandle, on: bool) {
    if VERTICAL_TABS.swap(on, Ordering::SeqCst) != on {
        let _ = app.emit("vertical-tabs", on);
        layout_all(app);
    }
}

pub fn bookmarks_bar_shown() -> bool {
    SHOW_BOOKMARKS_BAR.load(Ordering::SeqCst)
}

/// Shows or hides the bookmarks bar in every window.
pub fn set_bookmarks_bar(app: &AppHandle, show: bool) {
    SHOW_BOOKMARKS_BAR.store(show, Ordering::SeqCst);
    let _ = app.emit("bookmarks-bar", show);
    layout_all(app);
}

/// Called when a tab webview is created in a window.
pub fn tab_created(app: &AppHandle, window: &str) {
    // On Windows the popup is brought to the front when shown (bring_to_front).
    if !cfg!(windows) {
        app.state::<Windows>().popup_behind.lock().unwrap().insert(window.to_string());
    }
}

/// Puts a webview above the window's other webviews (tabs made later are
/// drawn on top of older webviews).
#[cfg(windows)]
pub fn bring_to_front(webview: &Webview) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    let _ = webview.with_webview(|platform| unsafe {
        // Each webview sits in its own child window, the controller's parent.
        let mut hwnd = HWND::default();
        if platform.controller().ParentWindow(&mut hwnd).is_ok() && !hwnd.is_invalid() {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    });
}

#[cfg(not(windows))]
pub fn bring_to_front(_webview: &Webview) {}

/// Rounds a popup's corners (radius in logical pixels), so it looks like a card
/// floating over the page. Windows clips the popup's child window to the shape.
#[cfg(windows)]
pub fn round_corners(webview: &Webview, size: (f64, f64), radius: f64) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    let scale = webview.window().scale_factor().unwrap_or(1.0);
    let (width, height) = ((size.0 * scale).round() as i32, (size.1 * scale).round() as i32);
    let diameter = (radius * 2.0 * scale).round() as i32;
    let _ = webview.with_webview(move |platform| unsafe {
        let mut hwnd = HWND::default();
        if platform.controller().ParentWindow(&mut hwnd).is_ok() && !hwnd.is_invalid() {
            if diameter <= 0 {
                SetWindowRgn(hwnd, None, true);
            } else {
                let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
                SetWindowRgn(hwnd, Some(region), true);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn round_corners(_webview: &Webview, _size: (f64, f64), _radius: f64) {}

/// Creates the window's popup webview, hidden, or returns the one there is.
fn make_popup(app: &AppHandle, window: &str, focus: bool) -> Result<Webview, String> {
    let label = popup_label(window);
    let behind = app.state::<Windows>().popup_behind.lock().unwrap().remove(window);
    match app.get_webview(&label) {
        Some(popup) if !behind => Ok(popup),
        other => {
            if let Some(old) = other {
                let _ = old.close();
            }
            let handle = app.get_window(window).ok_or("No window")?;
            let builder = extensions::allow(
                WebviewBuilder::new(&label, WebviewUrl::App("popup.html".into()))
                    .background_color(MENU_BACKGROUND)
                    .focused(focus),
            );
            let popup = handle
                .add_child(builder, LogicalPosition::new(0.0, 0.0), LogicalSize::new(10.0, 10.0))
                .map_err(|e| e.to_string())?;
            let _ = popup.hide();
            Ok(popup)
        }
    }
}

/// Sizes a window's toolbar and tabs to the window.
pub fn layout(app: &AppHandle, window: &str) {
    let Some(handle) = app.get_window(window) else { return };
    let scale = handle.scale_factor().unwrap_or(1.0);
    let Ok(size) = handle.inner_size() else { return };
    let size = size.to_logical::<f64>(scale);
    let state = with_windows(app, |states| states.get(window).cloned()).unwrap_or_default();
    let full = state.full_window;
    // The toolbar webview fills the window behind the page: its background is
    // the colour around the page card.
    if let Some(toolbar) = app.get_webview(&ui_label(window)) {
        let _ = toolbar.set_size(LogicalSize::new(size.width, size.height));
    }
    let (page, panel) = page_layout((size.width, size.height), toolbar_height(), left_space(), state.ai_panel, state.fullscreen);
    let radius = if state.fullscreen { 0.0 } else { PAGE_RADIUS };
    let split_label = state.split.filter(|_| !state.fullscreen).map(|id| format!("tab-{id}"));
    let (left, right) = if split_label.is_some() { split_halves(page) } else { (page, page) };
    for webview in tabs::webviews_of(app, window) {
        let rect = if Some(webview.label()) == split_label.as_deref() { right } else { left };
        let _ = webview.set_position(LogicalPosition::new(rect.x, rect.y));
        // Tabs sit on top of the toolbar webview, so while the toolbar fills the
        // window (history, settings) they are shrunk to nothing, not only hidden.
        let (width, height) = if full { (0.0, 0.0) } else { (rect.width, rect.height) };
        let _ = webview.set_size(LogicalSize::new(width, height));
        round_corners(&webview, (width, height), radius);
    }
    crate::ai::place(app, window, if full { None } else { panel });
}

pub fn is_fullscreen(app: &AppHandle, window: &str) -> bool {
    with_windows(app, |states| states.get(window).is_some_and(|s| s.fullscreen))
}

/// F11 or a page asking for full screen (a video).
pub fn set_fullscreen(app: &AppHandle, window: &str, on: bool) {
    with_windows(app, |states| {
        if let Some(state) = states.get_mut(window) {
            state.fullscreen = on;
        }
    });
    if let Some(handle) = app.get_window(window) {
        let _ = handle.set_fullscreen(on);
    }
    close_popup(app, window);
    let _ = app.emit("fullscreen", serde_json::json!({ "window": window, "on": on }));
    layout(app, window);
}

pub fn ai_panel_open(app: &AppHandle, window: &str) -> bool {
    with_windows(app, |states| states.get(window).is_some_and(|s| s.ai_panel))
}

pub fn set_ai_panel(app: &AppHandle, window: &str, open: bool) {
    with_windows(app, |states| {
        if let Some(state) = states.get_mut(window) {
            state.ai_panel = open;
        }
    });
    let _ = app.emit("ai-panel", serde_json::json!({ "window": window, "open": open }));
    layout(app, window);
}

pub fn layout_all(app: &AppHandle) {
    for window in window_labels(app) {
        layout(app, &window);
    }
}

/// Opens a window. `restore` reopens saved tabs (asleep except the active one);
/// without it the window starts with a new tab page.
pub fn open_window(
    app: &AppHandle,
    private: bool,
    restore: Option<&session::SavedWindow>,
) -> tauri::Result<String> {
    let number = NEXT_WINDOW.fetch_add(1, Ordering::SeqCst);
    let label = format!("win-{number}");
    with_windows(app, |states| {
        states.insert(label.clone(), WindowState { private, ..Default::default() })
    });

    let width = 1280.0;
    let height = 800.0;
    let mut builder = WindowBuilder::new(app, &label)
        .title(if private { "Raven (gizli)" } else { "Raven" })
        .inner_size(width, height)
        .min_inner_size(640.0, 400.0)
        // No Windows title bar: Raven's toolbar has its own window buttons.
        .decorations(false)
        .shadow(true)
        .background_color(BACKGROUND);
    // New windows open a little lower and to the right of the first one, like Chrome.
    if number > 1 {
        let offset = 32.0 * ((number - 1) % 8) as f64;
        builder = builder.position(80.0 + offset, 60.0 + offset);
    }
    let window = builder.build()?;

    let toolbar = extensions::allow(
        WebviewBuilder::new(ui_label(&label), WebviewUrl::App("index.html".into())).background_color(BACKGROUND),
    );
    window.add_child(toolbar, LogicalPosition::new(0.0, 0.0), LogicalSize::new(width, height))?;

    match restore.filter(|saved| !saved.tabs.is_empty()) {
        Some(saved) => {
            let ids: Vec<u32> = saved
                .tabs
                .iter()
                .map(|tab| tabs::add_sleeping_tab(app, &label, &tab.url, &tab.title, tab.pinned, tab.last_used))
                .collect();
            let members: Vec<(u32, u32)> = saved
                .tabs
                .iter()
                .zip(&ids)
                .filter_map(|(tab, id)| tab.group.map(|g| (*id, g)))
                .collect();
            tabs::restore_groups(app, &label, &saved.groups, &members);
            if let Some(active) = ids.get(saved.active).or(ids.first()).copied() {
                tabs::show_only(app, active);
            }
        }
        None => {
            tabs::create_tab(app, &label, tabs::NEW_TAB, true)?;
        }
    }
    // Load the popup page now, so menus open at once later.
    let _ = make_popup(app, &label, false);

    let events = app.clone();
    let name = label.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) => {
            layout(&events, &name);
            close_popup(&events, &name);
        }
        WindowEvent::CloseRequested { .. } => {
            // Closing the last normal window with "clear on exit": wipe now,
            // while a webview can still do it (and again at the next start).
            let last = window_labels(&events).iter().filter(|w| !is_private(&events, w)).count() <= 1;
            if last && events.state::<crate::settings::SettingsState>().0.lock().unwrap().clear_on_exit {
                crate::history::clear_now(&events);
                if let Some(toolbar) = events.get_webview(&ui_label(&name)) {
                    let _ = toolbar.clear_all_browsing_data();
                }
            }
        }
        WindowEvent::Destroyed => window_closed(&events, &name),
        _ => {}
    });

    Ok(label)
}

/// A window was closed: forget it and its tabs. When it was the last window,
/// Raven is quitting, so the session keeps its tabs for the next start.
fn window_closed(app: &AppHandle, window: &str) {
    let others_left = with_windows(app, |states| {
        states.remove(window);
        !states.is_empty()
    });
    app.state::<Windows>().popup_behind.lock().unwrap().remove(window);
    app.state::<Windows>().last_popup.lock().unwrap().remove(window);
    tabs::forget_window(app, window, others_left);
}

/// Opens a new window (Ctrl+N) or a new private window (Ctrl+Shift+N).
pub fn new_window(app: &AppHandle, private: bool) -> Result<String, String> {
    let label = open_window(app, private, None).map_err(|e| e.to_string())?;
    if let Some(window) = app.get_window(&label) {
        let _ = window.set_focus();
    }
    Ok(label)
}

/// Lets a window's toolbar fill the whole window (open = true) or go back to a bar.
pub fn set_full_window(app: &AppHandle, window: &str, open: bool) {
    with_windows(app, |states| {
        if let Some(state) = states.get_mut(window) {
            state.full_window = open;
        }
    });
    if open {
        tabs::hide_all(app, window);
        layout(app, window);
    } else {
        layout(app, window);
        tabs::show_active(app, window);
    }
}

// Popups: one small webview per window, drawn over the page.

/// What a popup shows, and where. Sent to ui/popup.js.
#[derive(Clone, Serialize)]
pub struct PopupRequest {
    pub window: String,
    pub kind: String,
    pub data: serde_json::Value,
    /// Take the keyboard (not for address suggestions).
    pub focus: bool,
    /// Size in logical pixels.
    pub width: f64,
    pub height: f64,
}

/// Opens the window's popup at a place (in the window, logical pixels) and size,
/// and tells it what to show. `focus` is false for the address suggestions, which
/// must leave the cursor in the address bar.
pub fn open_popup(
    app: &AppHandle,
    window: &str,
    request: PopupRequest,
    position: (f64, f64),
    size: (f64, f64),
    focus: bool,
) -> Result<(), String> {
    let handle = app.get_window(window).ok_or("No window")?;
    let scale = handle.scale_factor().map_err(|e| e.to_string())?;
    let window_size = handle.inner_size().map_err(|e| e.to_string())?.to_logical::<f64>(scale);
    // Keep it inside the window.
    let x = position.0.min(window_size.width - size.0 - 4.0).max(4.0);
    let y = position.1.max(0.0);
    let place = LogicalPosition::new(x, y);
    let dimensions = LogicalSize::new(size.0, size.1);

    let popup = make_popup(app, window, focus)?;
    popup.set_position(place).map_err(|e| e.to_string())?;
    popup.set_size(dimensions).map_err(|e| e.to_string())?;
    // The popup page draws it, then calls popup_ready, which shows it. So the
    // old content never flashes. A page still loading asks with get_popup_request.
    app.state::<Windows>().last_popup.lock().unwrap().insert(window.to_string(), request.clone());
    let _ = app.emit("popup-show", &request);
    Ok(())
}

/// The popup page has drawn what was asked: show it on top.
fn show_popup(app: &AppHandle, window: &str) {
    let Some(popup) = app.get_webview(&popup_label(window)) else { return };
    let Some(request) = app.state::<Windows>().last_popup.lock().unwrap().get(window).cloned() else {
        return;
    };
    let focus = request.focus;
    round_corners(&popup, (request.width, request.height), 14.0);
    bring_to_front(&popup);
    let _ = popup.show();
    if focus {
        let _ = popup.set_focus();
    }
}

pub fn close_popup(app: &AppHandle, window: &str) {
    if let Some(popup) = app.get_webview(&popup_label(window)) {
        let _ = popup.hide();
        let _ = app.emit("popup-closed", window);
    }
}

#[derive(Serialize)]
pub struct WindowInfo {
    label: String,
    private: bool,
}

#[tauri::command]
pub async fn get_window_info(app: AppHandle, webview: Webview) -> WindowInfo {
    let label = window_of(&webview);
    WindowInfo { private: is_private(&app, &label), label }
}

#[tauri::command]
pub async fn set_full_window_command(app: AppHandle, webview: Webview, open: bool) {
    set_full_window(&app, &window_of(&webview), open);
}

#[tauri::command]
pub async fn close_popup_command(app: AppHandle, webview: Webview) {
    close_popup(&app, &window_of(&webview));
}

/// Opens a popup from the toolbar: kind, where (x, y in the window) and size.
#[tauri::command]
pub async fn open_popup_command(
    app: AppHandle,
    webview: Webview,
    kind: String,
    data: serde_json::Value,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    focus: bool,
) -> Result<(), String> {
    let window = window_of(&webview);
    let request = PopupRequest { window: window.clone(), kind, data, focus, width, height };
    open_popup(&app, &window, request, (x, y), (width, height), focus)
}

/// Sends something to a window's open popup (for example which address
/// suggestion is highlighted while you press the arrow keys).
#[tauri::command]
pub async fn popup_update(app: AppHandle, webview: Webview, data: serde_json::Value) {
    let window = window_of(&webview);
    let _ = app.emit("popup-update", serde_json::json!({ "window": window, "data": data }));
}

#[tauri::command]
pub async fn popup_ready(app: AppHandle, webview: Webview) {
    show_popup(&app, &window_of(&webview));
}

#[tauri::command]
pub async fn get_popup_request(app: AppHandle, webview: Webview) -> Option<PopupRequest> {
    let window = window_of(&webview);
    app.state::<Windows>().last_popup.lock().unwrap().get(&window).cloned()
}

#[tauri::command]
pub async fn get_bookmarks_bar() -> bool {
    bookmarks_bar_shown()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_card_layout() {
        let (page, panel) = page_layout((1280.0, 800.0), 84.0, 8.0, false, false);
        assert_eq!(page, Rect { x: 8.0, y: 84.0, width: 1264.0, height: 708.0 });
        assert!(panel.is_none());
        let (page, panel) = page_layout((1280.0, 800.0), 84.0, 8.0, true, false);
        let panel = panel.unwrap();
        assert_eq!(page.width + panel.width + 3.0 * PAGE_MARGIN, 1280.0);
        assert_eq!(panel.x + panel.width, 1272.0);
        // Too narrow for the panel: the page keeps the space.
        assert!(page_layout((600.0, 800.0), 84.0, 8.0, true, false).1.is_none());
        let (page, _) = page_layout((1280.0, 800.0), 84.0, 8.0, true, true);
        assert_eq!(page, Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 });
        let (left, right) = split_halves(Rect { x: 8.0, y: 84.0, width: 1008.0, height: 700.0 });
        assert_eq!((left.width, right.x, right.width), (500.0, 516.0, 500.0));
        // Vertical tabs: the list takes the left side.
        let (page, _) = page_layout((1280.0, 800.0), 44.0, 240.0, false, false);
        assert_eq!(page, Rect { x: 240.0, y: 44.0, width: 1032.0, height: 748.0 });
        let (page, _) = page_layout((1280.0, 800.0), 84.0, 8.0, true, true);
        assert_eq!(page, Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 });
    }
}
