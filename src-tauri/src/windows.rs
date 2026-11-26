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
pub const BACKGROUND: tauri::window::Color = tauri::window::Color(17, 19, 23, 255);
/// The popup menus' color (--menu in ui/theme.css).
const MENU_BACKGROUND: tauri::window::Color = tauri::window::Color(29, 32, 39, 255);

static SHOW_BOOKMARKS_BAR: AtomicBool = AtomicBool::new(false);
static NEXT_WINDOW: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Default)]
struct WindowState {
    private: bool,
    /// The toolbar fills the whole window (history, downloads, settings).
    full_window: bool,
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
    if SHOW_BOOKMARKS_BAR.load(Ordering::SeqCst) {
        TOOLBAR_BASE + BOOKMARKS_BAR
    } else {
        TOOLBAR_BASE
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
fn bring_to_front(webview: &Webview) {
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
fn bring_to_front(_webview: &Webview) {}

/// Rounds a popup's corners (radius in logical pixels), so it looks like a card
/// floating over the page. Windows clips the popup's child window to the shape.
#[cfg(windows)]
fn round_corners(webview: &Webview, size: (f64, f64), radius: f64) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    let scale = webview.window().scale_factor().unwrap_or(1.0);
    let (width, height) = ((size.0 * scale).round() as i32, (size.1 * scale).round() as i32);
    let diameter = (radius * 2.0 * scale).round() as i32;
    let _ = webview.with_webview(move |platform| unsafe {
        let mut hwnd = HWND::default();
        if platform.controller().ParentWindow(&mut hwnd).is_ok() && !hwnd.is_invalid() {
            let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
            SetWindowRgn(hwnd, Some(region), true);
        }
    });
}

#[cfg(not(windows))]
fn round_corners(_webview: &Webview, _size: (f64, f64), _radius: f64) {}

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
    let full = is_full_window(app, window);
    let top = toolbar_height();
    if let Some(toolbar) = app.get_webview(&ui_label(window)) {
        let height = if full { size.height } else { top };
        let _ = toolbar.set_size(LogicalSize::new(size.width, height));
    }
    for webview in tabs::webviews_of(app, window) {
        let _ = webview.set_position(LogicalPosition::new(0.0, top));
        // Tabs sit on top of the toolbar webview, so while the toolbar fills the
        // window they are also shrunk to nothing, not only hidden.
        let (width, height) =
            if full { (0.0, 0.0) } else { (size.width, (size.height - top).max(0.0)) };
        let _ = webview.set_size(LogicalSize::new(width, height));
    }
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
        states.insert(label.clone(), WindowState { private, full_window: false })
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
    window.add_child(toolbar, LogicalPosition::new(0.0, 0.0), LogicalSize::new(width, toolbar_height()))?;

    match restore.filter(|saved| !saved.tabs.is_empty()) {
        Some(saved) => {
            let ids: Vec<u32> = saved
                .tabs
                .iter()
                .map(|tab| tabs::add_sleeping_tab(app, &label, &tab.url, &tab.title, tab.pinned))
                .collect();
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
    round_corners(&popup, (request.width, request.height), 12.0);
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
