//! The AI panel: Claude, ChatGPT or Gemini at the right of the page, with the
//! user's own account (their website, no API key). Raven can hand it the page
//! you are reading: "Özetle", "Açıkla", "Çevir" or your own question.

use serde::Serialize;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, Webview, WebviewUrl,
};

use crate::{extensions, native, settings::SettingsState, tabs, windows};

/// id, name, address.
pub const PROVIDERS: &[(&str, &str, &str)] = &[
    ("claude", "Claude", "https://claude.ai/new"),
    ("chatgpt", "ChatGPT", "https://chatgpt.com/"),
    ("gemini", "Gemini", "https://gemini.google.com/app"),
];

/// The toolbar draws the panel's header (AI picker, quick actions) above the
/// AI's page.
pub const HEADER: f64 = 48.0;

fn label(window: &str) -> String {
    format!("ai-{window}")
}

fn provider_url(id: &str) -> &'static str {
    PROVIDERS.iter().find(|(p, _, _)| *p == id).unwrap_or(&PROVIDERS[0]).2
}

fn current_provider(app: &AppHandle) -> String {
    app.state::<SettingsState>().0.lock().unwrap().ai_provider.clone()
}

/// Puts the panel where the layout says, or hides it.
pub fn place(app: &AppHandle, window: &str, rect: Option<windows::Rect>) {
    let existing = app.get_webview(&label(window));
    let Some(rect) = rect else {
        if let Some(panel) = existing {
            let _ = panel.hide();
        }
        return;
    };
    let panel = match existing {
        Some(panel) => panel,
        None => match create(app, window) {
            Ok(panel) => panel,
            Err(error) => {
                eprintln!("AI panel: {error}");
                return;
            }
        },
    };
    let size = (rect.width, (rect.height - HEADER).max(0.0));
    let _ = panel.set_position(LogicalPosition::new(rect.x, rect.y + HEADER));
    let _ = panel.set_size(LogicalSize::new(size.0, size.1));
    windows::round_corners(&panel, size, windows::PAGE_RADIUS);
    let _ = panel.show();
}

fn create(app: &AppHandle, window: &str) -> Result<Webview, String> {
    let handle = app.get_window(window).ok_or("No window")?;
    let url = provider_url(&current_provider(app)).parse().map_err(|_| "Bad address")?;
    let private = windows::is_private(app, window);
    let opener = app.clone();
    let owner = window.to_string();
    let builder = extensions::allow(WebviewBuilder::new(label(window), WebviewUrl::External(url)))
        .background_color(windows::BACKGROUND)
        .incognito(private)
        // Links from the AI open as tabs next to the page.
        .on_new_window(move |url, _features| {
            let app = opener.clone();
            let window = owner.clone();
            let url = url.to_string();
            tauri::async_runtime::spawn(async move {
                let _ = tabs::open_tab(&app, &window, &url);
            });
            NewWindowResponse::Deny
        });
    handle
        .add_child(builder, LogicalPosition::new(0.0, 0.0), LogicalSize::new(10.0, 10.0))
        .map_err(|e| e.to_string())
}

/// The panel's webview of the window a command came from.
fn panel_of(app: &AppHandle, window: &str) -> Option<Webview> {
    app.get_webview(&label(window))
}

#[derive(Serialize)]
pub struct Provider {
    id: &'static str,
    name: &'static str,
}

#[tauri::command]
pub async fn get_ai_providers() -> Vec<Provider> {
    PROVIDERS.iter().map(|(id, name, _)| Provider { id, name }).collect()
}

#[tauri::command]
pub async fn toggle_ai_panel(app: AppHandle, webview: Webview, open: Option<bool>) {
    let window = windows::window_of(&webview);
    let open = open.unwrap_or(!windows::ai_panel_open(&app, &window));
    windows::set_ai_panel(&app, &window, open);
}

/// Switches the AI (saved in the settings) and opens it in every panel.
#[tauri::command]
pub async fn set_ai_provider(app: AppHandle, provider: String) -> Result<(), String> {
    if !PROVIDERS.iter().any(|(id, _, _)| *id == provider) {
        return Err("Unknown AI".into());
    }
    let settings = {
        let state = app.state::<SettingsState>();
        let mut settings = state.0.lock().unwrap();
        settings.ai_provider = provider.clone();
        settings.clone()
    };
    crate::settings::store(&app, &settings)?;
    let url: tauri::Url = provider_url(&provider).parse().map_err(|_| "Bad address")?;
    for window in windows::window_labels(&app) {
        if let Some(panel) = panel_of(&app, &window) {
            let _ = panel.navigate(url.clone());
        }
    }
    use tauri::Emitter;
    let _ = app.emit("settings", &settings);
    Ok(())
}

/// Reloads the AI's page (for example after signing in).
#[tauri::command]
pub async fn reload_ai(app: AppHandle, webview: Webview) {
    if let Some(panel) = panel_of(&app, &windows::window_of(&webview)) {
        let _ = panel.eval("location.reload()");
    }
}

/// Text of the page in a window's active tab: (title, address, text).
fn page_of(app: &AppHandle, window: &str) -> Option<(String, String, String)> {
    let (url, title) = tabs::active_page(app, window)?;
    let text = tabs::active_webview(app, window)
        .and_then(|tab| native::run_script(&tab, PAGE_TEXT).ok())
        // The script's result is JSON of a string.
        .and_then(|json| serde_json::from_str::<String>(&json).ok())
        .unwrap_or_default();
    Some((title, url, text))
}

/// The readable text of a page, without menus and footers where possible.
const PAGE_TEXT: &str = r#"(() => {
  const main = document.querySelector("article, main, [role=main]") || document.body;
  const text = (main ? main.innerText : "").replace(/\n{3,}/g, "\n\n").trim();
  return text.slice(0, 15000);
})()"#;

/// Puts text in the AI's message box and sends it. The page may still be
/// loading, so the script waits for the box.
fn fill_script(text: &str) -> String {
    let text = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(async (text) => {{
  const find = () => document.querySelector('div.ProseMirror[contenteditable="true"], #prompt-textarea, rich-textarea .ql-editor, div[contenteditable="true"], textarea');
  let box = null;
  for (let i = 0; i < 80 && !(box = find()); i++) await new Promise((r) => setTimeout(r, 250));
  if (!box) return;
  box.focus();
  if (box.tagName === "TEXTAREA") {{
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
    setter.call(box, text);
    box.dispatchEvent(new Event("input", {{ bubbles: true }}));
  }} else {{
    document.execCommand("selectAll", false);
    document.execCommand("insertText", false, text);
  }}
  await new Promise((r) => setTimeout(r, 500));
  const send = document.querySelector('button[data-testid="send-button"], button[aria-label*="Send" i], button[aria-label*="Gönder" i], button[aria-label*="ileti" i], button.send-button');
  if (send && !send.disabled) send.click();
  else box.dispatchEvent(new KeyboardEvent("keydown", {{ key: "Enter", code: "Enter", keyCode: 13, bubbles: true }}));
}})({text});"#
    )
}

/// The message Raven writes for a quick action.
pub fn prompt(action: &str, question: Option<&str>, title: &str, url: &str, text: &str) -> String {
    let ask = match action {
        "summarize" => "Bu sayfayı Türkçe olarak kısaca özetle. Önce bir cümlelik özet, sonra en önemli noktalar.".to_string(),
        "explain" => "Bu sayfayı basit bir dille, teknik bilgisi olmayan birine anlatır gibi açıkla.".to_string(),
        "translate" => "Bu sayfanın metnini Türkçeye çevir.".to_string(),
        _ => question.unwrap_or("Bu sayfa hakkında ne söyleyebilirsin?").to_string(),
    };
    if text.is_empty() {
        format!("{ask}\n\nSayfa: {title}\n{url}")
    } else {
        format!("{ask}\n\nSayfa: {title}\n{url}\n\n---\n{text}")
    }
}

/// "Özetle", "Açıkla", "Çevir" or a question about the page: opens the panel
/// and sends the page to the AI.
#[tauri::command]
pub async fn ask_ai(app: AppHandle, webview: Webview, action: String, question: Option<String>) -> Result<(), String> {
    let window = windows::window_of(&webview);
    let (title, url, text) = {
        let app = app.clone();
        let window = window.clone();
        tauri::async_runtime::spawn_blocking(move || page_of(&app, &window))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Açık bir sayfa yok")?
    };
    if url.is_empty() {
        return Err("Önce bir sayfa aç".into());
    }
    if !windows::ai_panel_open(&app, &window) {
        windows::set_ai_panel(&app, &window, true);
    }
    let panel = panel_of(&app, &window).ok_or("Yapay zeka paneli açılamadı")?;
    let message = prompt(&action, question.as_deref(), &title, &url, &text);
    panel.eval(fill_script(&message)).map_err(|e| e.to_string())?;
    let _ = panel.set_focus();
    Ok(())
}

/// Opens the AI panel and sends it a question (or just opens it when empty).
pub fn ask_direct(app: &AppHandle, window: &str, question: &str) {
    if !windows::ai_panel_open(app, window) {
        windows::set_ai_panel(app, window, true);
    }
    if let Some(panel) = panel_of(app, window) {
        if !question.trim().is_empty() {
            let _ = panel.eval(fill_script(question.trim()));
        }
        let _ = panel.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_include_the_page() {
        let message = prompt("summarize", None, "Başlık", "https://a.com/", "metin");
        assert!(message.contains("özetle") && message.contains("https://a.com/") && message.ends_with("metin"));
        let message = prompt("ask", Some("Kim yazdı?"), "B", "https://b.com/", "");
        assert!(message.starts_with("Kim yazdı?") && !message.contains("---"));
    }

    #[test]
    fn fill_script_escapes_text() {
        let script = fill_script("a\"b</script>");
        assert!(script.contains(r#""a\"b</script>""#));
    }
}
