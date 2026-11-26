//! Raven Browser.
//!
//! Each window (windows.rs) holds its toolbar webview, a popup webview for menus
//! and its tabs (tabs.rs). Websites in tabs get no access to Raven's commands.

mod bookmarks;
mod downloads;
mod history;
mod media;
mod permissions;
mod session;
mod settings;
mod shortcuts;
mod storage;
mod suggest;
mod tabs;
mod windows;
mod zoom;

use std::sync::Mutex;

use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(tabs::Tabs::default())
        .manage(windows::Windows::default())
        .invoke_handler(tauri::generate_handler![
            tabs::get_tabs,
            tabs::close_tab,
            tabs::switch_tab,
            tabs::move_tab_command,
            tabs::tab_action,
            tabs::open_url,
            tabs::navigate,
            tabs::go_back,
            tabs::go_forward,
            tabs::reload,
            shortcuts::run_action,
            settings::get_settings,
            settings::save_settings,
            settings::get_search_engines,
            settings::clear_browsing_data,
            bookmarks::get_bookmarks,
            bookmarks::toggle_bookmark,
            bookmarks::remove_bookmark,
            history::get_history,
            history::get_top_sites,
            history::remove_history_entry,
            history::clear_history,
            downloads::get_downloads,
            downloads::open_download,
            downloads::show_download_in_folder,
            downloads::remove_download,
            downloads::clear_downloads,
            permissions::get_site_permissions,
            permissions::set_site_permission,
            permissions::reset_site_permissions,
            suggest::get_suggestions,
            windows::get_window_info,
            windows::set_full_window_command,
            windows::open_popup_command,
            windows::close_popup_command,
            windows::get_popup_request,
            windows::popup_update,
            windows::get_bookmarks_bar,
        ])
        .setup(|app| {
            let handle = app.handle();
            let saved = settings::load(handle);
            let restore = saved.restore_session;
            app.manage(settings::SettingsState(Mutex::new(saved)));
            app.manage(bookmarks::load(handle));
            app.manage(history::load(handle));
            app.manage(downloads::load(handle));
            app.manage(zoom::load(handle));
            app.manage(permissions::load(handle));
            history::start_saving(handle);
            let last_session = session::load(handle);
            bookmarks::refresh_bar(handle);

            // Reopen the last windows and tabs, or start with one new window.
            let saved_windows = if restore { last_session.saved_windows() } else { Vec::new() };
            if saved_windows.is_empty() {
                windows::open_window(handle, false, None)?;
            } else {
                for saved in &saved_windows {
                    windows::open_window(handle, false, Some(saved))?;
                }
            }
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
