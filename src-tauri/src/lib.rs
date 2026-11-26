//! Raven Browser: the main window.
//!
//! One window holds two webviews:
//! - "ui": Raven's own toolbar (back, forward, reload, address bar), from `ui/`.
//! - "content": the website the user is browsing. It has no access to Raven's commands.

use tauri::{
    webview::{PageLoadEvent, WebviewBuilder},
    window::WindowBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl,
    WindowEvent,
};

const TOOLBAR_HEIGHT: f64 = 60.0;
const HOME_URL: &str = "https://www.google.com";
const SEARCH_URL: &str = "https://www.google.com/search";

/// Turns what the user typed into a web address: a URL, a domain, or a search.
fn to_url(input: &str) -> Result<Url, String> {
    let text = input.trim();
    if text.is_empty() {
        return HOME_URL.parse().map_err(|e| format!("{e}"));
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return text.parse().map_err(|e| format!("{e}"));
    }
    if text.contains('.') && !text.contains(' ') {
        if let Ok(url) = format!("https://{text}").parse() {
            return Ok(url);
        }
    }
    Url::parse_with_params(SEARCH_URL, &[("q", text)]).map_err(|e| format!("{e}"))
}

fn content(app: &AppHandle) -> Result<Webview, String> {
    app.get_webview("content")
        .ok_or_else(|| "Content area not found".to_string())
}

#[tauri::command]
fn navigate(app: AppHandle, input: String) -> Result<(), String> {
    let url = to_url(&input)?;
    content(&app)?.navigate(url).map_err(|e| e.to_string())
}

#[tauri::command]
fn go_back(app: AppHandle) -> Result<(), String> {
    content(&app)?
        .eval("history.back()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn go_forward(app: AppHandle) -> Result<(), String> {
    content(&app)?
        .eval("history.forward()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reload(app: AppHandle) -> Result<(), String> {
    content(&app)?
        .eval("location.reload()")
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![navigate, go_back, go_forward, reload])
        .setup(|app| {
            let width = 1280.0;
            let height = 800.0;

            let window = WindowBuilder::new(app, "main")
                .title("Raven")
                .inner_size(width, height)
                .min_inner_size(640.0, 400.0)
                .build()?;

            // Raven's toolbar.
            let toolbar = WebviewBuilder::new("ui", WebviewUrl::App("index.html".into()));
            window.add_child(
                toolbar,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(width, TOOLBAR_HEIGHT),
            )?;

            // The website area. Tells the toolbar which address is open.
            let handle = app.handle().clone();
            let site = WebviewBuilder::new("content", WebviewUrl::External(HOME_URL.parse()?))
                .on_page_load(move |_webview, payload| {
                    let state = match payload.event() {
                        PageLoadEvent::Started => "loading",
                        PageLoadEvent::Finished => "done",
                    };
                    let _ = handle.emit(
                        "content-page",
                        serde_json::json!({ "url": payload.url().as_str(), "state": state }),
                    );
                });
            window.add_child(
                site,
                LogicalPosition::new(0.0, TOOLBAR_HEIGHT),
                LogicalSize::new(width, height - TOOLBAR_HEIGHT),
            )?;

            // Keep the toolbar and the website area sized to the window.
            let win = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Resized(size) = event {
                    let scale = win.scale_factor().unwrap_or(1.0);
                    let size = size.to_logical::<f64>(scale);
                    if let Some(toolbar) = win.get_webview("ui") {
                        let _ = toolbar.set_size(LogicalSize::new(size.width, TOOLBAR_HEIGHT));
                    }
                    if let Some(site) = win.get_webview("content") {
                        let _ = site.set_position(LogicalPosition::new(0.0, TOOLBAR_HEIGHT));
                        let _ = site.set_size(LogicalSize::new(
                            size.width,
                            (size.height - TOOLBAR_HEIGHT).max(0.0),
                        ));
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Raven could not start");
}

#[cfg(test)]
mod tests {
    use super::to_url;

    #[test]
    fn keeps_full_urls() {
        assert_eq!(to_url("https://github.com").unwrap().as_str(), "https://github.com/");
    }

    #[test]
    fn adds_https_to_domains() {
        assert_eq!(to_url("f2pclub.app").unwrap().as_str(), "https://f2pclub.app/");
    }

    #[test]
    fn searches_plain_text() {
        let url = to_url("raven browser").unwrap();
        assert_eq!(url.host_str(), Some("www.google.com"));
        assert_eq!(url.query(), Some("q=raven+browser"));
    }

    #[test]
    fn empty_goes_home() {
        assert_eq!(to_url("  ").unwrap().as_str(), "https://www.google.com/");
    }
}
