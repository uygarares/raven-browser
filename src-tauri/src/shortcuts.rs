//! Keyboard shortcuts, the same as in Chrome and Edge.
//!
//! They work wherever the focus is: in Raven's toolbar (ui/main.js sends them
//! with `run_action`) and inside websites (on Windows, `listen_in_tab` catches
//! them before the website sees them).

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::tabs;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    NewTab,
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
    NewPrivateTab,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleBookmark,
    ToggleBookmarksBar,
}

/// Matches a key press to an action, like Chrome: Ctrl+T, Ctrl+W, Ctrl+Shift+T,
/// Ctrl+L, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageDown, Ctrl+PageUp, Ctrl+1..8, Ctrl+9,
/// Ctrl+H (history), Ctrl+D (bookmark), Ctrl+Shift+B (bookmarks bar).
/// `key` is the Windows virtual key code.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn from_key(ctrl: bool, shift: bool, key: u32) -> Option<Action> {
    if !ctrl {
        return None;
    }
    let action = match (key, shift) {
        (0x54, false) => Action::NewTab,      // T
        (0x54, true) => Action::ReopenTab,    // Shift+T
        (0x57, false) => Action::CloseTab,    // W
        (0x4C, false) => Action::FocusAddress, // L
        (0x09, false) | (0x22, false) => Action::NextTab, // Tab, PageDown
        (0x09, true) | (0x21, false) => Action::PreviousTab, // Shift+Tab, PageUp
        (0x31..=0x38, false) => Action::GoToTab((key - 0x31) as usize), // 1..8
        (0x39, false) => Action::LastTab,     // 9
        (0x48, false) => Action::OpenHistory, // H
        (0x4A, false) => Action::OpenDownloads, // J
        (0x4E, true) => Action::NewPrivateTab, // Shift+N
        (0xBB, _) | (0x6B, _) => Action::ZoomIn, // = + and numpad +
        (0xBD, false) | (0x6D, false) => Action::ZoomOut, // - and numpad -
        (0x30, false) | (0x60, false) => Action::ZoomReset, // 0 and numpad 0
        (0x44, false) => Action::ToggleBookmark, // D
        (0x42, true) => Action::ToggleBookmarksBar, // Shift+B
        _ => return None,
    };
    Some(action)
}

/// Puts the cursor in the address bar and selects its text.
fn focus_address(app: &AppHandle) {
    if let Some(toolbar) = app.get_webview("ui") {
        let _ = toolbar.set_focus();
    }
    let _ = app.emit("focus-address", ());
}

/// Runs an action. Safe to call from any thread, including webview event handlers.
pub fn perform(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match action {
            Action::NewTab => tabs::open_tab(&app, tabs::NEW_TAB).map(|_| focus_address(&app)),
            Action::CloseTab => tabs::close_active(&app),
            Action::ReopenTab => tabs::reopen_closed(&app),
            Action::FocusAddress => {
                focus_address(&app);
                Ok(())
            }
            Action::NextTab => tabs::switch_relative(&app, 1),
            Action::PreviousTab => tabs::switch_relative(&app, -1),
            Action::GoToTab(index) => tabs::switch_to_index(&app, index),
            Action::LastTab => tabs::switch_to_last(&app),
            Action::OpenHistory => {
                if let Some(toolbar) = app.get_webview("ui") {
                    let _ = toolbar.set_focus();
                }
                let _ = app.emit("open-history", ());
                Ok(())
            }
            Action::OpenSettings => {
                if let Some(toolbar) = app.get_webview("ui") {
                    let _ = toolbar.set_focus();
                }
                let _ = app.emit("open-settings", ());
                Ok(())
            }
            Action::OpenDownloads => {
                if let Some(toolbar) = app.get_webview("ui") {
                    let _ = toolbar.set_focus();
                }
                let _ = app.emit("open-downloads", ());
                Ok(())
            }
            Action::NewPrivateTab => tabs::open_private_tab(&app).map(|_| focus_address(&app)),
            Action::ZoomIn => tabs::zoom_active(&app, 1),
            Action::ZoomOut => tabs::zoom_active(&app, -1),
            Action::ZoomReset => tabs::zoom_active(&app, 0),
            Action::ToggleBookmark => crate::bookmarks::toggle_active(&app).map(|_| ()),
            Action::ToggleBookmarksBar => crate::bookmarks::toggle_bar(&app),
        };
        if let Err(error) = result {
            eprintln!("Shortcut {action:?} failed: {error}");
        }
    });
}

#[tauri::command]
pub async fn run_action(app: AppHandle, action: Action) {
    perform(&app, action);
}

/// Catches Raven's shortcuts inside a website tab, before the website gets them.
#[cfg(windows)]
pub fn listen_in_tab(app: &AppHandle, webview: &tauri::Webview) {
    use webview2_com::{AcceleratorKeyPressedEventHandler, Microsoft::Web::WebView2::Win32::*};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};

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
                args.SetHandled(true)?;
                perform(&app, action);
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = controller.add_AcceleratorKeyPressed(&handler, &mut token);
    });
}

/// Other systems: shortcuts inside websites are not caught yet.
#[cfg(not(windows))]
pub fn listen_in_tab(_app: &AppHandle, _webview: &tauri::Webview) {}

#[cfg(test)]
mod tests {
    use super::{from_key, Action};

    #[test]
    fn chrome_shortcuts() {
        assert_eq!(from_key(true, false, 0x54), Some(Action::NewTab));
        assert_eq!(from_key(true, true, 0x54), Some(Action::ReopenTab));
        assert_eq!(from_key(true, false, 0x57), Some(Action::CloseTab));
        assert_eq!(from_key(true, false, 0x4C), Some(Action::FocusAddress));
        assert_eq!(from_key(true, false, 0x09), Some(Action::NextTab));
        assert_eq!(from_key(true, true, 0x09), Some(Action::PreviousTab));
        assert_eq!(from_key(true, false, 0x31), Some(Action::GoToTab(0)));
        assert_eq!(from_key(true, false, 0x38), Some(Action::GoToTab(7)));
        assert_eq!(from_key(true, false, 0x39), Some(Action::LastTab));
        assert_eq!(from_key(true, false, 0x48), Some(Action::OpenHistory));
        assert_eq!(from_key(true, true, 0x4E), Some(Action::NewPrivateTab));
        assert_eq!(from_key(true, false, 0xBB), Some(Action::ZoomIn));
        assert_eq!(from_key(true, true, 0xBB), Some(Action::ZoomIn)); // Ctrl + Shift + = is Ctrl + +
        assert_eq!(from_key(true, false, 0xBD), Some(Action::ZoomOut));
        assert_eq!(from_key(true, false, 0x30), Some(Action::ZoomReset));
        assert_eq!(from_key(true, false, 0x44), Some(Action::ToggleBookmark));
        assert_eq!(from_key(true, true, 0x42), Some(Action::ToggleBookmarksBar));
    }

    #[test]
    fn ignores_other_keys() {
        assert_eq!(from_key(false, false, 0x54), None); // T without Ctrl
        assert_eq!(from_key(true, false, 0x41), None); // Ctrl+A stays with the website
        assert_eq!(from_key(true, false, 0x43), None); // Ctrl+C
    }
}
