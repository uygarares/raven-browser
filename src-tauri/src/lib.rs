//! Raven Browser: the main window.
//!
//! One window holds several webviews:
//! - "ui": Raven's own toolbar (tabs, back, forward, reload, address bar), from `ui/`.
//! - "tab-N": one per tab, showing a website. Only the active tab is visible.
//!   Websites get no access to Raven's commands.

mod tabs;

use tauri::{window::WindowBuilder, LogicalPosition, LogicalSize, Manager, WindowEvent};

/// Height of the toolbar, in logical pixels. Must match `--toolbar-height` in ui/style.css.
pub const TOOLBAR_HEIGHT: f64 = 84.0;

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
        ])
        .setup(|app| {
            let width = 1280.0;
            let height = 800.0;

            let window = WindowBuilder::new(app, "main")
                .title("Raven")
                .inner_size(width, height)
                .min_inner_size(640.0, 400.0)
                // No Windows title bar: Raven's toolbar has its own window buttons.
                .decorations(false)
                .shadow(true)
                .build()?;

            // Raven's toolbar.
            let toolbar = tauri::webview::WebviewBuilder::new(
                "ui",
                tauri::WebviewUrl::App("index.html".into()),
            );
            window.add_child(
                toolbar,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(width, TOOLBAR_HEIGHT),
            )?;

            // The first tab.
            tabs::create_tab(app.handle(), tabs::HOME_URL, true)?;

            // Keep the toolbar and every tab sized to the window.
            let win = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Resized(size) = event {
                    let scale = win.scale_factor().unwrap_or(1.0);
                    let size = size.to_logical::<f64>(scale);
                    for (label, webview) in win.app_handle().webviews() {
                        if label == "ui" {
                            let _ = webview.set_size(LogicalSize::new(size.width, TOOLBAR_HEIGHT));
                        } else if tabs::is_tab_label(&label) {
                            let _ = webview.set_position(LogicalPosition::new(0.0, TOOLBAR_HEIGHT));
                            let _ = webview.set_size(LogicalSize::new(
                                size.width,
                                (size.height - TOOLBAR_HEIGHT).max(0.0),
                            ));
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Raven could not start");
}
