//! Keyboard shortcuts and menu actions, the same as in Chrome and Edge.
//!
//! They work wherever the focus is: in Raven's toolbar and menus (ui/*.js send
//! them with `run_action`) and inside websites (on Windows, `listen_in_tab`
//! catches them before the website sees them). Each action applies to the
//! window it came from.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Webview};

use crate::{bookmarks, tabs, windows};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    NewTab,
    NewWindow,
    NewPrivateWindow,
    CloseTab,
    ReopenTab,
    FocusAddress,
    NextTab,
    PreviousTab,
    /// Tab number 1 to 8, counted from the left (0-based here).
    GoToTab(usize),
    LastTab,
    OpenHistory,
    OpenDownloads,
    OpenSettings,
    OpenExtensions,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleBookmark,
    ToggleBookmarksBar,
    /// Ctrl+F: find in page.
    Find,
    /// Ctrl+Space: open or close the AI panel.
    ToggleAi,
    /// F11.
    Fullscreen,
    /// F12 or Ctrl+Shift+I.
    DevTools,
    /// Ctrl+K: search tabs, history, bookmarks and commands.
    CommandPalette,
    /// Ctrl+Shift+A: search open tabs.
    TabSearch,
    /// Ctrl+P (from Raven's toolbar; inside pages WebView2 prints by itself).
    Print,
    /// The reading view of the page.
    ReaderMode,
    OpenArchive,
    /// Ctrl+Shift+O.
    OpenBookmarks,
    /// Put the other tabs to sleep now.
    SleepOtherTabs,
}

/// Matches a key press to an action, like Chrome: Ctrl+T, Ctrl+N, Ctrl+Shift+N,
/// Ctrl+W, Ctrl+Shift+T, Ctrl+L, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageDown,
/// Ctrl+PageUp, Ctrl+1..8, Ctrl+9, Ctrl+H, Ctrl+J, Ctrl+D, Ctrl+Shift+B,
/// Ctrl + plus / minus / 0. `key` is the Windows virtual key code.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn from_key(ctrl: bool, shift: bool, key: u32) -> Option<Action> {
    if !ctrl {
        return match key {
            0x7A => Some(Action::Fullscreen), // F11
            0x7B => Some(Action::DevTools),   // F12
            _ => None,
        };
    }
    let action = match (key, shift) {
        (0x46, false) => Action::Find,           // F
        (0x20, false) => Action::ToggleAi,       // Space
        (0x4B, false) => Action::CommandPalette, // K
        (0x41, true) => Action::TabSearch,       // Shift+A
        (0x49, true) => Action::DevTools,        // Shift+I
        (0x4F, true) => Action::OpenBookmarks,   // Shift+O
        (0x54, false) => Action::NewTab,      // T
        (0x54, true) => Action::ReopenTab,    // Shift+T
        (0x4E, false) => Action::NewWindow,   // N
        (0x4E, true) => Action::NewPrivateWindow, // Shift+N
        (0x57, false) => Action::CloseTab,    // W
        (0x4C, false) => Action::FocusAddress, // L
        (0x09, false) | (0x22, false) => Action::NextTab, // Tab, PageDown
        (0x09, true) | (0x21, false) => Action::PreviousTab, // Shift+Tab, PageUp
        (0x31..=0x38, false) => Action::GoToTab((key - 0x31) as usize), // 1..8
        (0x39, false) => Action::LastTab,     // 9
        (0x48, false) => Action::OpenHistory, // H
        (0x4A, false) => Action::OpenDownloads, // J
        (0xBB, _) | (0x6B, _) => Action::ZoomIn, // = + and numpad +
        (0xBD, false) | (0x6D, false) => Action::ZoomOut, // - and numpad -
        (0x30, false) | (0x60, false) => Action::ZoomReset, // 0 and numpad 0
        (0x44, false) => Action::ToggleBookmark, // D
        (0x42, true) => Action::ToggleBookmarksBar, // Shift+B
        _ => return None,
    };
    Some(action)
}

/// Something a window's toolbar should do, sent as an event with the window's name.
#[derive(Clone, Serialize)]
struct ToolbarRequest<'a> {
    window: &'a str,
}

/// Gives the keyboard to a window's toolbar and asks it to do something
/// ("focus-address", "open-history", "open-downloads", "open-settings").
fn tell_toolbar(app: &AppHandle, window: &str, event: &str) {
    if let Some(toolbar) = app.get_webview(&windows::ui_label(window)) {
        let _ = toolbar.set_focus();
    }
    let _ = app.emit(event, ToolbarRequest { window });
}

/// Runs an action for a window. Safe to call from any thread, including
/// webview event handlers.
pub fn perform(app: &AppHandle, window: String, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let window = window.as_str();
        let result = match action {
            Action::NewTab => {
                tabs::open_tab(&app, window, tabs::NEW_TAB).map(|_| tell_toolbar(&app, window, "focus-address"))
            }
            Action::NewWindow => windows::new_window(&app, false).map(|_| ()),
            Action::NewPrivateWindow => windows::new_window(&app, true).map(|_| ()),
            Action::CloseTab => tabs::close_active(&app, window),
            Action::ReopenTab => tabs::reopen_closed(&app, window),
            Action::FocusAddress => {
                tell_toolbar(&app, window, "focus-address");
                Ok(())
            }
            Action::NextTab => tabs::switch_relative(&app, window, 1),
            Action::PreviousTab => tabs::switch_relative(&app, window, -1),
            Action::GoToTab(index) => tabs::switch_to_index(&app, window, index),
            Action::LastTab => tabs::switch_to_last(&app, window),
            Action::OpenHistory => {
                tell_toolbar(&app, window, "open-history");
                Ok(())
            }
            Action::OpenDownloads => {
                tell_toolbar(&app, window, "open-downloads");
                Ok(())
            }
            Action::OpenSettings => {
                tell_toolbar(&app, window, "open-settings");
                Ok(())
            }
            Action::OpenExtensions => {
                tell_toolbar(&app, window, "open-extensions");
                Ok(())
            }
            Action::ZoomIn => tabs::zoom_active(&app, window, 1),
            Action::ZoomOut => tabs::zoom_active(&app, window, -1),
            Action::ZoomReset => tabs::zoom_active(&app, window, 0),
            Action::ToggleBookmark => bookmarks::toggle_active(&app, window).map(|_| ()),
            Action::ToggleBookmarksBar => bookmarks::toggle_bar(&app),
            Action::Find => {
                tell_toolbar(&app, window, "open-find");
                Ok(())
            }
            Action::ToggleAi => {
                let open = !windows::ai_panel_open(&app, window);
                windows::set_ai_panel(&app, window, open);
                Ok(())
            }
            Action::Fullscreen => {
                let on = !windows::is_fullscreen(&app, window);
                if !on {
                    // A video in full screen leaves it too.
                    if let Some(tab) = tabs::active_webview(&app, window) {
                        let _ = tab.eval("document.fullscreenElement && document.exitFullscreen()");
                    }
                }
                windows::set_fullscreen(&app, window, on);
                Ok(())
            }
            Action::DevTools => {
                if let Some(tab) = tabs::active_webview(&app, window) {
                    tab.open_devtools();
                }
                Ok(())
            }
            Action::CommandPalette => {
                tell_toolbar(&app, window, "open-palette");
                Ok(())
            }
            Action::TabSearch => {
                tell_toolbar(&app, window, "open-tab-search");
                Ok(())
            }
            Action::Print => tabs::active_webview(&app, window)
                .ok_or_else(|| "No page".to_string())
                .and_then(|tab| tab.print().map_err(|e| e.to_string())),
            Action::ReaderMode => crate::reader::toggle(&app, window),
            Action::OpenBookmarks => {
                tell_toolbar(&app, window, "open-bookmarks");
                Ok(())
            }
            Action::OpenArchive => {
                tell_toolbar(&app, window, "open-archive");
                Ok(())
            }
            Action::SleepOtherTabs => {
                tabs::sleep_others(&app, window);
                Ok(())
            }
        };
        if let Err(error) = result {
            eprintln!("Action {action:?} failed: {error}");
        }
    });
}

#[tauri::command]
pub async fn run_action(app: AppHandle, webview: Webview, action: Action) {
    perform(&app, windows::window_of(&webview), action);
}

/// Catches Raven's shortcuts inside a website tab, before the website gets them.
#[cfg(windows)]
pub fn listen_in_tab(app: &AppHandle, webview: &tauri::Webview, id: u32) {
    use webview2_com::{AcceleratorKeyPressedEventHandler, Microsoft::Web::WebView2::Win32::*};
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};

    let app = app.clone();
    let _ = webview.with_webview(move |platform| unsafe {
        let controller = platform.controller();
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| unsafe {
            let Some(args) = args else { return Ok(()) };

            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                return Ok(());
            }

            // Ignore auto-repeat when the key is held down.
            let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
            args.PhysicalKeyStatus(&mut status)?;
            if status.WasKeyDown.as_bool() {
                return Ok(());
            }

            let mut key = 0u32;
            args.VirtualKey(&mut key)?;
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;

            if let Some(action) = from_key(ctrl, shift, key) {
                if let Some(window) = tabs::window_of_tab(&app, id) {
                    args.SetHandled(true)?;
                    perform(&app, window, action);
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = controller.add_AcceleratorKeyPressed(&handler, &mut token);
    });
}

/// Other systems: shortcuts inside websites are not caught yet.
#[cfg(not(windows))]
pub fn listen_in_tab(_app: &AppHandle, _webview: &tauri::Webview, _id: u32) {}

#[cfg(test)]
mod tests {
    use super::{from_key, Action};

    #[test]
    fn chrome_shortcuts() {
        assert_eq!(from_key(true, false, 0x54), Some(Action::NewTab));
        assert_eq!(from_key(true, true, 0x54), Some(Action::ReopenTab));
        assert_eq!(from_key(true, false, 0x4E), Some(Action::NewWindow));
        assert_eq!(from_key(true, true, 0x4E), Some(Action::NewPrivateWindow));
        assert_eq!(from_key(true, false, 0x57), Some(Action::CloseTab));
        assert_eq!(from_key(true, false, 0x4C), Some(Action::FocusAddress));
        assert_eq!(from_key(true, false, 0x09), Some(Action::NextTab));
        assert_eq!(from_key(true, true, 0x09), Some(Action::PreviousTab));
        assert_eq!(from_key(true, false, 0x31), Some(Action::GoToTab(0)));
        assert_eq!(from_key(true, false, 0x38), Some(Action::GoToTab(7)));
        assert_eq!(from_key(true, false, 0x39), Some(Action::LastTab));
        assert_eq!(from_key(true, false, 0x48), Some(Action::OpenHistory));
        assert_eq!(from_key(true, false, 0xBB), Some(Action::ZoomIn));
        assert_eq!(from_key(true, true, 0xBB), Some(Action::ZoomIn)); // Ctrl + Shift + = is Ctrl + +
        assert_eq!(from_key(true, false, 0xBD), Some(Action::ZoomOut));
        assert_eq!(from_key(true, false, 0x30), Some(Action::ZoomReset));
        assert_eq!(from_key(true, false, 0x44), Some(Action::ToggleBookmark));
        assert_eq!(from_key(true, true, 0x42), Some(Action::ToggleBookmarksBar));
        assert_eq!(from_key(true, false, 0x46), Some(Action::Find));
        assert_eq!(from_key(true, false, 0x20), Some(Action::ToggleAi));
        assert_eq!(from_key(false, false, 0x7A), Some(Action::Fullscreen));
        assert_eq!(from_key(false, false, 0x7B), Some(Action::DevTools));
        assert_eq!(from_key(true, false, 0x4B), Some(Action::CommandPalette));
        assert_eq!(from_key(true, true, 0x41), Some(Action::TabSearch));
        assert_eq!(from_key(false, false, 0x54), None);
    }

    #[test]
    fn ignores_other_keys() {
        assert_eq!(from_key(false, false, 0x54), None); // T without Ctrl
        assert_eq!(from_key(true, false, 0x41), None); // Ctrl+A stays with the website
        assert_eq!(from_key(true, false, 0x43), None); // Ctrl+C
    }
}
