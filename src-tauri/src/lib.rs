//! Raven Browser: the main window.
//!
//! One see-through window holds several webviews:
//! - "ui": Raven's own toolbar (tabs, back, forward, reload, address bar), from `ui/`.
//!   It grows to fill the window for full-window screens like the theme picker.
//! - "tab-N": one per tab, showing a website or Raven's new tab page. Only the
//!   active tab is visible. Tabs get no access to Raven's commands.
//!
//! The window itself is transparent, so the glass theme shows the desktop behind
//! Raven's own parts. Websites paint their own background as usual.

mod settings;
mod shortcuts;
mod tabs;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{window::WindowBuilder, AppHandle, LogicalPosition, LogicalSize, Manager, WindowEvent};

/// Height of the toolbar, in logical pixels. Must match `--toolbar-height` in ui/style.css.
pub const TOOLBAR_HEIGHT: f64 = 84.0;

/// Fully see-through. WebView2 only accepts fully clear or fully solid backgrounds.
pub const CLEAR: tauri::window::Color = tauri::window::Color(0, 0, 0, 0);

/// True while the toolbar fills the whole window (for example the theme picker).
static FULL_WINDOW_UI: AtomicBool = AtomicBool::new(false);

pub fn is_full_window() -> bool {
    FULL_WINDOW_UI.load(Ordering::SeqCst)
}

/// Sizes the toolbar and every tab to the window.
fn layout(app: &AppHandle) {
    let Some(window) = app.get_window("main") else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(size) = window.inner_size() else { return };
    let size = size.to_logical::<f64>(scale);
    let full = FULL_WINDOW_UI.load(Ordering::SeqCst);
    for (label, webview) in app.webviews() {
        if label == "ui" {
            let height = if full { size.height } else { TOOLBAR_HEIGHT };
            let _ = webview.set_size(LogicalSize::new(size.width, height));
        } else if tabs::is_tab_label(&label) {
            let _ = webview.set_position(LogicalPosition::new(0.0, TOOLBAR_HEIGHT));
            let _ = webview.set_size(LogicalSize::new(
                size.width,
                (size.height - TOOLBAR_HEIGHT).max(0.0),
            ));
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(tabs::Tabs::default())
        .invoke_handler(tauri::generate_handler![
            tabs::get_tabs,
            tabs::new_tab,
            tabs::close_tab,
            tabs::switch_tab,
            tabs::navigate,
            tabs::go_back,
            tabs::go_forward,
            tabs::reload,
            shortcuts::run_action,
            settings::get_settings,
            settings::save_settings,
            set_full_window,
        ])
        .setup(|app| {
            let saved = settings::load(app.handle());
            app.manage(settings::SettingsState(Mutex::new(saved)));

            let width = 1280.0;
            let height = 800.0;

            let window = WindowBuilder::new(app, "main")
                .title("Raven")
                .inner_size(width, height)
                .min_inner_size(640.0, 400.0)
                // No Windows title bar: Raven's toolbar has its own window buttons.
                .decorations(false)
                .shadow(true)
                // See-through, for the glass theme. The solid theme paints over it.
                .transparent(true)
                .background_color(CLEAR)
                .build()?;

            // Raven's toolbar.
            let toolbar = tauri::webview::WebviewBuilder::new(
                "ui",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .transparent(true)
            .background_color(CLEAR);
            window.add_child(
                toolbar,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(width, TOOLBAR_HEIGHT),
            )?;

            // The first tab.
            tabs::create_tab(app.handle(), tabs::NEW_TAB, true)?;

            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Resized(_) = event {
                    layout(&handle);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Raven could not start");
}
