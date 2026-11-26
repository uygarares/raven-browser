//! Raven Browser.
//!
//! Each window (windows.rs) holds its toolbar webview, a popup webview for menus
//! and its tabs (tabs.rs). Websites in tabs get no access to Raven's commands.

// Windows callbacks run inside unsafe blocks; nested ones are fine.
#![cfg_attr(windows, allow(unused_unsafe))]

mod ai;
mod bookmarks;
mod default_browser;
mod downloads;
mod extensions;
mod find;
mod history;
mod import;
mod media;
mod native;
mod perf;
mod permissions;
mod reader;
mod session;
mod settings;
mod shield;
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
        // A second Raven (a link clicked in another program) hands its address
        // to the one already running and quits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match default_browser::url_in_args(&args) {
                    Some(url) => default_browser::open_from_outside(&app, &url),
                    None => {
                        let _ = windows::new_window(&app, false);
                    }
                }
            });
        }))
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
            bookmarks::edit_bookmark,
            bookmarks::move_bookmark,
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
            extensions::get_extensions,
            extensions::install_extension,
            extensions::get_adblock,
            extensions::install_adblock,
            extensions::set_extension_enabled,
            extensions::remove_extension,
            extensions::open_extension_options,
            extensions::open_extension_store,
            perf::get_archive,
            perf::open_archived,
            perf::clear_archive,
            perf::get_memory,
            reader::toggle_reader,
            import::get_import_sources,
            import::import_browser,
            shield::get_shield,
            shield::set_shield_for_site,
            shield::update_shield_lists,
            find::find_in_page,
            find::stop_finding,
            default_browser::is_default_browser,
            default_browser::make_default_browser,
            ai::get_ai_providers,
            ai::toggle_ai_panel,
            ai::set_ai_provider,
            ai::reload_ai,
            ai::ask_ai,
            windows::get_window_info,
            windows::set_full_window_command,
            windows::open_popup_command,
            windows::close_popup_command,
            windows::get_popup_request,
            windows::popup_ready,
            windows::popup_update,
            windows::get_bookmarks_bar,
        ])
        .setup(|app| {
            let handle = app.handle();
            let saved = settings::load(handle);
            let restore = saved.restore_session;
            let restore_autoplay = saved.block_autoplay;
            let vertical = saved.vertical_tabs;
            app.manage(settings::SettingsState(Mutex::new(saved)));
            app.manage(bookmarks::load(handle));
            app.manage(history::load(handle));
            app.manage(downloads::load(handle));
            app.manage(zoom::load(handle));
            app.manage(permissions::load(handle));
            app.manage(extensions::load(handle));
            app.manage(perf::load(handle));
            app.manage(shield::load(handle));
            extensions::BLOCK_AUTOPLAY.store(restore_autoplay, std::sync::atomic::Ordering::Relaxed);
            windows::set_vertical_tabs(handle, vertical);
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
            // Started with a link (Raven is the default browser).
            let args: Vec<String> = std::env::args().collect();
            if let Some(url) = default_browser::url_in_args(&args) {
                default_browser::open_from_outside(handle, &url);
            }
            extensions::start(handle);
            perf::start(handle);
            shield::start(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Raven could not start");

    app.run(|app, event| {
        // Save the history that has not been written yet.
        if let RunEvent::Exit = event {
            history::flush(app);
            shield::flush(app);
        }
    });
}
