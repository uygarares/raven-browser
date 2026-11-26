//! Raven Browser: the main window.
//!
//! One window holds several webviews:
//! - "ui": Raven's own toolbar (tabs, back, forward, reload, address bar,
//!   bookmarks bar), from `ui/`. It grows to fill the window for full-window
//!   panels like the history.
//! - "tab-N": one per awake tab, showing a website or Raven's new tab page. Only
//!   the active tab is visible. Tabs get no access to Raven's commands.

mod bookmarks;
mod downloads;
mod history;
mod session;
mod settings;
mod shortcuts;
mod storage;
mod tabs;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{
    window::WindowBuilder, AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, RunEvent,
    WindowEvent,
};

/// Height of the tab row and address bar, in logical pixels.
/// Must match `--toolbar-height` in ui/style.css.
const TOOLBAR_BASE: f64 = 84.0;
/// Extra height while the bookmarks bar is shown. Must match `--bookmarks-bar-height`.
const BOOKMARKS_BAR: f64 = 32.0;

/// Raven's dark background (the Solid theme's page color). Shown for a moment
/// before a page draws itself, instead of a white flash.
pub const BACKGROUND: tauri::window::Color = tauri::window::Color(11, 13, 20, 255);

/// True while the toolbar fills the whole window (for example the history panel).
static FULL_WINDOW_UI: AtomicBool = AtomicBool::new(false);
/// True while the bookmarks bar is shown.
static SHOW_BOOKMARKS_BAR: AtomicBool = AtomicBool::new(false);

pub fn is_full_window() -> bool {
    FULL_WINDOW_UI.load(Ordering::SeqCst)
}

/// Where the website area starts.
pub fn toolbar_height() -> f64 {
    if SHOW_BOOKMARKS_BAR.load(Ordering::SeqCst) {
        TOOLBAR_BASE + BOOKMARKS_BAR
    } else {
        TOOLBAR_BASE
    }
}

/// Shows or hides the bookmarks bar and moves the websites to make room.
pub fn set_bookmarks_bar(app: &AppHandle, show: bool) {
    SHOW_BOOKMARKS_BAR.store(show, Ordering::SeqCst);
    let _ = app.emit("bookmarks-bar", show);
    layout(app);
}

/// Sizes the toolbar and every tab to the window.
pub fn layout(app: &AppHandle) {
    let Some(window) = app.get_window("main") else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(size) = window.inner_size() else { return };
    let size = size.to_logical::<f64>(scale);
    let full = is_full_window();
    let top = toolbar_height();
    for (label, webview) in app.webviews() {
        if label == "ui" {
            let height = if full { size.height } else { top };
            let _ = webview.set_size(LogicalSize::new(size.width, height));
        } else if tabs::is_tab_label(&label) {
            let _ = webview.set_position(LogicalPosition::new(0.0, top));
            // Tabs sit on top of the toolbar webview, so while the toolbar fills the
            // window they are also shrunk to nothing, not only hidden.
            let (width, height) =
                if full { (0.0, 0.0) } else { (size.width, (size.height - top).max(0.0)) };
            let _ = webview.set_size(LogicalSize::new(width, height));
        }
    }
}

/// Lets the toolbar fill the whole window (open = true) or go back to a bar.
#[tauri::command]
async fn set_full_window(app: AppHandle, open: bool) {
    FULL_WINDOW_UI.store(open, Ordering::SeqCst);
    if open {
        tabs::hide_all(&app);
        layout(&app);
    } else {
        layout(&app);
        tabs::show_active(&app);
    }
}

/// Is the bookmarks bar shown? Asked by the toolbar when it loads.
#[tauri::command]
async fn get_bookmarks_bar() -> bool {
    SHOW_BOOKMARKS_BAR.load(Ordering::SeqCst)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(tabs::Tabs::default())
        .invoke_handler(tauri::generate_handler![
            tabs::get_tabs,
            tabs::new_tab,
            tabs::close_tab,
            tabs::switch_tab,
            tabs::open_url,
            tabs::navigate,
            tabs::go_back,
            tabs::go_forward,
            tabs::reload,
            shortcuts::run_action,
            settings::get_settings,
            settings::save_settings,
            bookmarks::get_bookmarks,
            bookmarks::toggle_bookmark,
            bookmarks::remove_bookmark,
            history::get_history,
            history::get_top_sites,
            downloads::get_downloads,
            downloads::open_download,
            downloads::show_download_in_folder,
            downloads::remove_download,
            downloads::clear_downloads,
            settings::get_search_engines,
            settings::clear_browsing_data,
            history::remove_history_entry,
            history::clear_history,
            set_full_window,
            get_bookmarks_bar,
        ])
        .setup(|app| {
            let handle = app.handle();
            let saved = settings::load(handle);
            let restore = saved.restore_session;
            app.manage(settings::SettingsState(Mutex::new(saved)));
            app.manage(bookmarks::load(handle));
            app.manage(history::load(handle));
            app.manage(downloads::load(handle));
            history::start_saving(handle);
            let last_session = session::load(handle);
            bookmarks::refresh_bar(handle);

            let width = 1280.0;
            let height = 800.0;

            let window = WindowBuilder::new(app, "main")
                .title("Raven")
                .inner_size(width, height)
                .min_inner_size(640.0, 400.0)
                // No Windows title bar: Raven's toolbar has its own window buttons.
                .decorations(false)
                .shadow(true)
                .background_color(BACKGROUND)
                .build()?;

            // Raven's toolbar.
            let toolbar = tauri::webview::WebviewBuilder::new(
                "ui",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .background_color(BACKGROUND);
            window.add_child(
                toolbar,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(width, toolbar_height()),
            )?;

            // Reopen the last tabs, asleep except the active one. Or start with a new tab.
            if restore && !last_session.tabs.is_empty() {
                let ids: Vec<u32> = last_session
                    .tabs
                    .iter()
                    .map(|tab| tabs::add_sleeping_tab(handle, &tab.url, &tab.title))
                    .collect();
                let active = ids.get(last_session.active).or(ids.first()).copied();
                if let Some(active) = active {
                    tabs::show_only(handle, active);
                }
            } else {
                tabs::create_tab(handle, tabs::NEW_TAB, true)?;
            }

            let resize_handle = handle.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Resized(_) = event {
                    layout(&resize_handle);
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Raven could not start");

    app.run(|app, event| {
        // Save the history that has not been written yet.
        if let RunEvent::Exit = event {
            history::flush(app);
        }
    });
}
