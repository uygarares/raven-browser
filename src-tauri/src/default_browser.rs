//! Making Raven the default browser, and opening links from other programs.
//!
//! Windows asks the user to pick the default browser in its Settings; a
//! program can only register itself as a choice. Raven registers for http,
//! https and web files under the current user, then opens that Settings page.
//! Links clicked in other programs start Raven with the address; if Raven is
//! already open, the single-instance plugin hands it to the running Raven.

use tauri::{AppHandle, Manager};

use crate::{tabs, windows};

/// The first web address or file in a program's arguments.
pub fn url_in_args(args: &[String]) -> Option<String> {
    args.iter().skip(1).find_map(|arg| {
        let arg = arg.trim_matches('"');
        if arg.starts_with("http://") || arg.starts_with("https://") || arg.starts_with("file://") {
            return Some(arg.to_string());
        }
        let path = std::path::Path::new(arg);
        let web_file = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .is_some_and(|e| ["htm", "html", "pdf", "svg", "webp", "xhtml", "mht"].contains(&e.as_str()));
        if web_file && path.exists() {
            let full = std::fs::canonicalize(path).ok()?;
            let text = full.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
            return Some(format!("file:///{}", text.trim_start_matches('/')));
        }
        None
    })
}

/// Opens an address in the window used last (or a new one).
pub fn open_from_outside(app: &AppHandle, url: &str) {
    let window = windows::window_labels(app)
        .into_iter()
        .find(|w| app.get_window(w).and_then(|h| h.is_focused().ok()).unwrap_or(false))
        .or_else(|| windows::window_labels(app).into_iter().find(|w| !windows::is_private(app, w)));
    let window = match window {
        Some(window) => window,
        None => match windows::open_window(app, false, None) {
            Ok(window) => window,
            Err(_) => return,
        },
    };
    let _ = tabs::open_tab(app, &window, url);
    if let Some(handle) = app.get_window(&window) {
        let _ = handle.unminimize();
        let _ = handle.set_focus();
    }
}

#[cfg(windows)]
mod imp {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const PROG_ID: &str = "RavenURL";
    const CLIENT: &str = r"Software\Clients\StartMenuInternet\Raven";

    pub fn register() -> std::io::Result<()> {
        let exe = std::env::current_exe()?;
        let exe = exe.to_string_lossy().to_string();
        let open = format!("\"{exe}\" \"%1\"");
        let icon = format!("\"{exe}\",0");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        let (class, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}"))?;
        class.set_value("", &"Raven HTML Document")?;
        class.set_value("FriendlyTypeName", &"Raven")?;
        class.create_subkey("DefaultIcon")?.0.set_value("", &icon)?;
        class.create_subkey(r"shell\open\command")?.0.set_value("", &open)?;
        let (app_info, _) = class.create_subkey("Application")?;
        app_info.set_value("ApplicationName", &"Raven")?;
        app_info.set_value("ApplicationIcon", &icon)?;
        app_info.set_value("ApplicationCompany", &"Raven Browser")?;

        let (client, _) = hkcu.create_subkey(CLIENT)?;
        client.set_value("", &"Raven")?;
        client.create_subkey("DefaultIcon")?.0.set_value("", &icon)?;
        client.create_subkey(r"shell\open\command")?.0.set_value("", &format!("\"{exe}\""))?;
        let (caps, _) = client.create_subkey("Capabilities")?;
        caps.set_value("ApplicationName", &"Raven")?;
        caps.set_value("ApplicationDescription", &"Hızlı, az RAM kullanan, reklam engelleyen tarayıcı")?;
        caps.set_value("ApplicationIcon", &icon)?;
        let (urls, _) = caps.create_subkey("URLAssociations")?;
        for scheme in ["http", "https"] {
            urls.set_value(scheme, &PROG_ID)?;
        }
        let (files, _) = caps.create_subkey("FileAssociations")?;
        for ext in [".htm", ".html", ".pdf", ".svg", ".webp", ".xhtml"] {
            files.set_value(ext, &PROG_ID)?;
        }
        let (registered, _) = hkcu.create_subkey(r"Software\RegisteredApplications")?;
        registered.set_value("Raven", &format!(r"{CLIENT}\Capabilities"))?;
        Ok(())
    }

    pub fn is_default() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice")
            .and_then(|key| key.get_value::<String, _>("ProgId"))
            .is_ok_and(|id| id == PROG_ID)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn register() -> std::io::Result<()> {
        Err(std::io::Error::other("Only on Windows"))
    }
    pub fn is_default() -> bool {
        false
    }
}

#[tauri::command]
pub async fn is_default_browser() -> bool {
    imp::is_default()
}

/// Registers Raven and opens Windows' "default apps" page on Raven.
#[tauri::command]
pub async fn make_default_browser() -> Result<(), String> {
    imp::register().map_err(|e| format!("Kaydedilemedi: {e}"))?;
    tauri_plugin_opener::open_url("ms-settings:defaultapps?registeredAppUser=Raven", None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_in_arguments() {
        let args = vec!["raven.exe".to_string(), "--flag".into(), "https://github.com/".into()];
        assert_eq!(url_in_args(&args).as_deref(), Some("https://github.com/"));
        assert_eq!(url_in_args(&["raven.exe".to_string()]), None);
        assert_eq!(url_in_args(&["raven.exe".to_string(), "C:/yok/dosya.html".into()]), None);
    }
}
