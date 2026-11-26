//! Chrome extensions, through WebView2's extension support (Windows).
//!
//! An extension is downloaded from the Chrome Web Store as a .crx file, unpacked
//! into Raven's data folder (extensions/<store id>) and added to WebView2.
//! WebView2 keeps it installed between starts.
//!
//! On the first start Raven adds uBlock Origin Lite, the ad blocker.
//!
//! What works: content scripts, blocking rules (ad blockers), background
//! workers and options pages. Not yet: the small window an extension's toolbar
//! button opens (WebView2 has no toolbar for extensions).

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{webview::WebviewBuilder, AppHandle, Emitter, Manager, Runtime, Webview};

use crate::{storage, tabs, windows};

/// uBlock Origin Lite in the Chrome Web Store.
pub const ADBLOCK_ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";
const FILE: &str = "extensions.json";
const STORE: &str = "https://chromewebstore.google.com/";

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    /// The ad blocker was added once. If the user removes it, it stays removed.
    adblock_added: bool,
    installed: Vec<Installed>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    store_id: String,
    /// WebView2's id for it (used in chrome-extension:// addresses).
    id: String,
    name: String,
    /// The settings page, inside the extension.
    options: Option<String>,
    /// The icon as a data: address, for the extensions page.
    icon: Option<String>,
}

#[derive(Default)]
pub struct Extensions {
    saved: Mutex<Saved>,
    /// Store ids being downloaded or added now.
    busy: Mutex<HashSet<String>>,
    /// Why the ad blocker could not be added, if it could not.
    adblock_error: Mutex<Option<String>>,
}

pub fn load(app: &AppHandle) -> Extensions {
    Extensions { saved: Mutex::new(storage::read(app, FILE)), busy: Mutex::default(), adblock_error: Mutex::default() }
}

fn save(app: &AppHandle) {
    let saved = app.state::<Extensions>().saved.lock().unwrap().clone();
    if let Err(error) = storage::write(app, FILE, &saved) {
        eprintln!("Could not save extensions: {error}");
    }
}

/// Every Raven webview must have extensions turned on: WebView2 refuses to
/// start webviews with different settings in the same data folder.
pub fn allow<R: Runtime>(builder: WebviewBuilder<R>) -> WebviewBuilder<R> {
    builder.browser_extensions_enabled(true)
}

/// A Chrome Web Store id is 32 letters from a to p.
fn is_store_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// The store id in a Chrome Web Store link, or an id typed on its own.
pub fn store_id(input: &str) -> Option<String> {
    let text = input.trim();
    if is_store_id(text) {
        return Some(text.to_string());
    }
    let path = text.split(['?', '#']).next().unwrap_or("");
    path.split('/').rev().find(|part| is_store_id(part)).map(String::from)
}

/// Where the store serves an extension's .crx file.
fn download_url(store_id: &str) -> String {
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=131.0.0.0&acceptformat=crx2,crx3&x=id%3D{store_id}%26uc"
    )
}

/// The zip archive inside a .crx file (after its signature header).
pub fn zip_part(bytes: &[u8]) -> Result<&[u8], String> {
    let number = |at: usize| -> Result<usize, String> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| "Bozuk uzantı dosyası".to_string())
    };
    if bytes.starts_with(b"PK") {
        return Ok(bytes);
    }
    if !bytes.starts_with(b"Cr24") {
        return Err("Bu bir Chrome uzantısı dosyası değil".into());
    }
    let start = match number(4)? {
        2 => 16 + number(8)? + number(12)?,
        3 => 12 + number(8)?,
        version => return Err(format!("Bilinmeyen uzantı biçimi ({version})")),
    };
    bytes.get(start..).filter(|rest| rest.starts_with(b"PK")).ok_or_else(|| "Bozuk uzantı dosyası".into())
}

fn download(store_id: &str) -> Result<Vec<u8>, String> {
    response_of(&download_url(store_id))
}

/// The bytes at an address (up to 200 MB), over the system's TLS.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    response_of(url)
}

fn response_of(url: &str) -> Result<Vec<u8>, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(tls))
        .timeout(std::time::Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("İndirilemedi: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(200 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("İndirilemedi: {e}"))?;
    if bytes.is_empty() {
        return Err("Boş cevap geldi".into());
    }
    Ok(bytes)
}

fn unpack(crx: &[u8], folder: &Path) -> Result<(), String> {
    let zip = zip_part(crx)?;
    let _ = fs::remove_dir_all(folder);
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| e.to_string())?;
    archive.extract(folder).map_err(|e| e.to_string())
}

/// The options page and icon from an unpacked extension's manifest.json.
fn read_manifest(folder: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = fs::read_to_string(folder.join("manifest.json")) else {
        return (None, None);
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        return (None, None);
    };
    let options = manifest["options_ui"]["page"]
        .as_str()
        .or(manifest["options_page"].as_str())
        .map(|page| page.trim_start_matches('/').to_string());
    // The icon closest to 48 pixels.
    let icon = manifest["icons"].as_object().and_then(|icons| {
        icons
            .iter()
            .filter_map(|(size, file)| Some((size.parse::<i64>().ok()?, file.as_str()?)))
            .min_by_key(|(size, _)| (size - 48).abs())
            .and_then(|(_, file)| {
                let file = file.trim_start_matches('/');
                let bytes = fs::read(folder.join(file)).ok()?;
                let kind = match file.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
                    Some("svg") => "image/svg+xml",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("webp") => "image/webp",
                    Some("gif") => "image/gif",
                    _ => "image/png",
                };
                (bytes.len() < 500_000).then(|| format!("data:{kind};base64,{}", base64(&bytes)))
            })
    });
    (options, icon)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn folder_of(app: &AppHandle, store_id: &str) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("extensions").join(store_id))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress<'a> {
    store_id: &'a str,
    /// "downloading", "adding", "done" or "failed".
    state: &'a str,
    error: Option<String>,
}

fn progress(app: &AppHandle, store_id: &str, state: &str, error: Option<String>) {
    let _ = app.emit("extension-install", Progress { store_id, state, error });
}

/// Downloads an extension from the Chrome Web Store and adds it. Slow: call it
/// from a background thread.
pub fn install(app: &AppHandle, store_id: &str) -> Result<(), String> {
    let state = app.state::<Extensions>();
    if state.saved.lock().unwrap().installed.iter().any(|e| e.store_id == store_id) {
        return Err("Bu uzantı zaten ekli".into());
    }
    if !state.busy.lock().unwrap().insert(store_id.to_string()) {
        return Err("Bu uzantı zaten ekleniyor".into());
    }
    let result = (|| -> Result<(), String> {
        progress(app, store_id, "downloading", None);
        let crx = download(store_id)?;
        let folder = folder_of(app, store_id)?;
        unpack(&crx, &folder)?;
        progress(app, store_id, "adding", None);
        let found = native::add(app, &folder)?;
        let (options, icon) = read_manifest(&folder);
        let mut saved = state.saved.lock().unwrap();
        saved.installed.retain(|e| e.store_id != store_id);
        saved.installed.push(Installed {
            store_id: store_id.to_string(),
            id: found.id,
            name: found.name,
            options,
            icon,
        });
        if store_id == ADBLOCK_ID {
            saved.adblock_added = true;
        }
        Ok(())
    })();
    state.busy.lock().unwrap().remove(store_id);
    if store_id == ADBLOCK_ID {
        *state.adblock_error.lock().unwrap() = result.clone().err();
    }
    match &result {
        Ok(()) => {
            save(app);
            progress(app, store_id, "done", None);
            let _ = app.emit("extensions-changed", ());
        }
        Err(error) => progress(app, store_id, "failed", Some(error.clone())),
    }
    result
}

/// On the first start, adds the ad blocker (uBlock Origin Lite) in the background.
pub fn start(app: &AppHandle) {
    if !cfg!(windows) || app.state::<Extensions>().saved.lock().unwrap().adblock_added {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // Let the first window open first.
        std::thread::sleep(std::time::Duration::from_secs(3));
        if let Err(error) = install(&app, ADBLOCK_ID) {
            eprintln!("Could not add the ad blocker: {error}");
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    id: String,
    store_id: Option<String>,
    name: String,
    enabled: bool,
    has_options: bool,
    icon: Option<String>,
    /// The ad blocker Raven adds by itself.
    adblock: bool,
}

/// On extension pages of the Chrome Web Store, the store's own "Chrome'a ekle"
/// button becomes "Raven'a ekle" and adds the extension to Raven.
pub const STORE_SCRIPT: &str = r#"(() => {
  if (location.hostname !== "chromewebstore.google.com") return;
  function idOfPage() {
    const match = location.pathname.match(/\/detail\/(?:[^/]+\/)?([a-p]{32})/);
    return match ? match[1] : null;
  }
  // The store's add button: a button whose text names Chrome (any language).
  function isAddButton(button) {
    return button && !button.closest("header, nav") && /chrome|raven/i.test(button.textContent || "")
      && (button.textContent || "").length < 40;
  }
  function relabel() {
    if (!idOfPage() || !document.body) return;
    for (const button of document.querySelectorAll("button")) {
      if (!isAddButton(button)) continue;
      button.disabled = false;
      button.removeAttribute("aria-disabled");
      const walker = document.createTreeWalker(button, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        if (/chrome/i.test(node.nodeValue)) node.nodeValue = node.nodeValue.replace(/Chrome/g, "Raven");
      }
    }
  }
  addEventListener("click", (event) => {
    const button = event.target instanceof Element ? event.target.closest("button") : null;
    const id = idOfPage();
    if (!id || !isAddButton(button)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    location.href = "raven-action:install-extension:" + id;
  }, true);
  setInterval(relabel, 500);
  addEventListener("DOMContentLoaded", relabel);
})();"#;

/// A tab on the Chrome Web Store asked to add an extension (its "Raven'a ekle"
/// button). Opens the extensions panel, which asks the user to confirm.
pub fn request_from_store(app: &AppHandle, tab: &Webview, window: &str, title: &str, store_id: &str) {
    let on_store = tab.url().ok().and_then(|url| url.host_str().map(String::from)).as_deref()
        == Some("chromewebstore.google.com");
    if !on_store || !is_store_id(store_id) {
        return;
    }
    let title = title.trim_end_matches(" - Chrome Web Store").to_string();
    if let Some(toolbar) = app.get_webview(&windows::ui_label(window)) {
        let _ = toolbar.set_focus();
    }
    let _ = app.emit(
        "open-extensions",
        serde_json::json!({ "window": window, "install": { "storeId": store_id, "title": title } }),
    );
}

#[tauri::command]
pub async fn get_extensions(app: AppHandle) -> Result<Vec<ExtensionInfo>, String> {
    let found = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || native::list(&app)
    })
    .await
    .map_err(|e| e.to_string())??;
    let saved = app.state::<Extensions>().saved.lock().unwrap().clone();
    Ok(found
        .into_iter()
        .map(|item| {
            // Match by WebView2's id, else by name.
            let known = saved
                .installed
                .iter()
                .find(|e| e.id == item.id)
                .or_else(|| saved.installed.iter().find(|e| !item.name.is_empty() && e.name == item.name));
            // Read the icon again if it was not saved.
            let icon = known.and_then(|e| {
                e.icon.clone().or_else(|| folder_of(&app, &e.store_id).ok().and_then(|f| read_manifest(&f).1))
            });
            ExtensionInfo {
                name: if item.name.is_empty() { known.map(|e| e.name.clone()).unwrap_or_default() } else { item.name },
                store_id: known.map(|e| e.store_id.clone()),
                has_options: known.is_some_and(|e| e.options.is_some()),
                icon,
                adblock: known.is_some_and(|e| e.store_id == ADBLOCK_ID),
                enabled: item.enabled,
                id: item.id,
            }
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdblockInfo {
    /// WebView2's id, when it is added.
    id: Option<String>,
    enabled: bool,
    /// Being downloaded or added now.
    busy: bool,
    error: Option<String>,
    has_options: bool,
}

/// The ad blocker's state, for the shield in the toolbar.
#[tauri::command]
pub async fn get_adblock(app: AppHandle) -> Result<AdblockInfo, String> {
    let (installed, busy, error) = {
        let state = app.state::<Extensions>();
        let installed = state.saved.lock().unwrap().installed.iter().find(|e| e.store_id == ADBLOCK_ID).cloned();
        let busy = state.busy.lock().unwrap().contains(ADBLOCK_ID);
        let error = state.adblock_error.lock().unwrap().clone();
        (installed, busy, error)
    };
    let enabled = match &installed {
        Some(item) => {
            let id = item.id.clone();
            let found = tauri::async_runtime::spawn_blocking({
                let app = app.clone();
                move || native::list(&app)
            })
            .await
            .map_err(|e| e.to_string())??;
            found.iter().find(|f| f.id == id).is_some_and(|f| f.enabled)
        }
        None => false,
    };
    Ok(AdblockInfo {
        has_options: installed.as_ref().is_some_and(|e| e.options.is_some()),
        id: installed.map(|e| e.id),
        enabled,
        busy,
        error,
    })
}

/// Adds the ad blocker (again).
#[tauri::command]
pub async fn install_adblock(app: AppHandle) -> Result<(), String> {
    install_extension(app, ADBLOCK_ID.to_string()).await
}

/// Adds an extension from a Chrome Web Store link (or its id).
#[tauri::command]
pub async fn install_extension(app: AppHandle, input: String) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Uzantılar şimdilik sadece Windows'ta çalışıyor".into());
    }
    let store_id = store_id(&input).ok_or("Bu bir Chrome Web Mağazası linki değil")?;
    tauri::async_runtime::spawn_blocking(move || install(&app, &store_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_extension_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        native::set_enabled(&app, &id, enabled)?;
        let _ = app.emit("extensions-changed", ());
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_extension(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        native::remove(&app, &id)?;
        let removed = {
            let state = app.state::<Extensions>();
            let mut saved = state.saved.lock().unwrap();
            let removed: Vec<String> =
                saved.installed.iter().filter(|e| e.id == id).map(|e| e.store_id.clone()).collect();
            saved.installed.retain(|e| e.id != id);
            removed
        };
        for store_id in removed {
            if let Ok(folder) = folder_of(&app, &store_id) {
                let _ = fs::remove_dir_all(folder);
            }
        }
        save(&app);
        let _ = app.emit("extensions-changed", ());
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens an extension's settings page in a new tab.
#[tauri::command]
pub async fn open_extension_options(app: AppHandle, webview: Webview, id: String) -> Result<(), String> {
    let page = app
        .state::<Extensions>()
        .saved
        .lock()
        .unwrap()
        .installed
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| e.options.clone())
        .ok_or("Bu uzantının ayar sayfası yok")?;
    tabs::open_tab(&app, &windows::window_of(&webview), &format!("chrome-extension://{id}/{page}")).map(|_| ())
}

/// Opens the Chrome Web Store in a new tab.
#[tauri::command]
pub async fn open_extension_store(app: AppHandle, webview: Webview) -> Result<(), String> {
    tabs::open_tab(&app, &windows::window_of(&webview), STORE).map(|_| ())
}

/// An extension as WebView2 reports it.
#[allow(dead_code)]
pub struct Found {
    id: String,
    name: String,
    enabled: bool,
}

/// WebView2's extension functions (Windows only).
#[cfg(windows)]
mod native {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use ::windows::core::{Interface, BOOL, HSTRING, PWSTR};
    use tauri::{AppHandle, Manager};
    use webview2_com::{
        BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler, CoTaskMemPWSTR,
        Microsoft::Web::WebView2::Win32::*, ProfileAddBrowserExtensionCompletedHandler,
        ProfileGetBrowserExtensionsCompletedHandler,
    };

    use super::Found;

    type Reply<T> = mpsc::Sender<Result<T, String>>;

    fn text(error: ::windows::core::Error) -> String {
        error.to_string()
    }

    /// Runs `f` with the browser profile, on the main thread. Any Raven
    /// toolbar uses the same profile as normal tabs.
    fn with_profile<T: Send + 'static>(
        app: &AppHandle,
        f: impl FnOnce(ICoreWebView2Profile7, Reply<T>) + Send + 'static,
    ) -> Result<T, String> {
        let webview = app
            .webviews()
            .into_iter()
            .find(|(label, _)| label.starts_with("ui-"))
            .map(|(_, webview)| webview)
            .ok_or("Raven penceresi yok")?;
        let (tx, rx) = mpsc::channel();
        webview
            .with_webview(move |platform| unsafe {
                let profile = platform
                    .controller()
                    .CoreWebView2()
                    .and_then(|core| core.cast::<ICoreWebView2_13>())
                    .and_then(|core| core.Profile())
                    .and_then(|profile| profile.cast::<ICoreWebView2Profile7>());
                match profile {
                    Ok(profile) => f(profile, tx),
                    Err(error) => {
                        let _ = tx.send(Err(format!("Bu WebView2 sürümü uzantıları desteklemiyor ({error})")));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(20)).map_err(|_| "WebView2 cevap vermedi".to_string())?
    }

    fn take(value: PWSTR) -> String {
        CoTaskMemPWSTR::from(value).to_string()
    }

    unsafe fn read(extension: &ICoreWebView2BrowserExtension) -> ::windows::core::Result<Found> {
        unsafe {
            let mut id = PWSTR::null();
            extension.Id(&mut id)?;
            let mut name = PWSTR::null();
            extension.Name(&mut name)?;
            let mut enabled = BOOL::default();
            extension.IsEnabled(&mut enabled)?;
            Ok(Found { id: take(id), name: take(name), enabled: enabled.as_bool() })
        }
    }

    unsafe fn all(list: Option<ICoreWebView2BrowserExtensionList>) -> ::windows::core::Result<Vec<ICoreWebView2BrowserExtension>> {
        let mut out = Vec::new();
        if let Some(list) = list {
            unsafe {
                let mut count = 0u32;
                list.Count(&mut count)?;
                for index in 0..count {
                    out.push(list.GetValueAtIndex(index)?);
                }
            }
        }
        Ok(out)
    }

    pub fn list(app: &AppHandle) -> Result<Vec<Found>, String> {
        with_profile(app, |profile, tx| unsafe {
            let reply = tx.clone();
            let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |result, list| {
                let found = result.and_then(|_| unsafe {
                    all(list)?.iter().map(|extension| read(extension)).collect::<::windows::core::Result<Vec<_>>>()
                });
                let _ = reply.send(found.map_err(text));
                Ok(())
            }));
            if let Err(error) = profile.GetBrowserExtensions(&handler) {
                let _ = tx.send(Err(text(error)));
            }
        })
    }

    pub fn add(app: &AppHandle, folder: &Path) -> Result<Found, String> {
        let path = HSTRING::from(folder);
        with_profile(app, move |profile, tx| unsafe {
            let reply = tx.clone();
            let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |result, extension| {
                let found = result.and_then(|_| match extension {
                    Some(extension) => unsafe { read(&extension) },
                    None => Err(::windows::core::Error::empty()),
                });
                let _ = reply.send(found.map_err(|e| format!("Uzantı eklenemedi: {e}")));
                Ok(())
            }));
            if let Err(error) = profile.AddBrowserExtension(&path, &handler) {
                let _ = tx.send(Err(format!("Uzantı eklenemedi: {error}")));
            }
        })
    }

    /// Finds an extension by id and runs `then` on it (on the main thread).
    fn with_extension(
        app: &AppHandle,
        id: &str,
        then: impl FnOnce(ICoreWebView2BrowserExtension, Reply<()>) + Send + 'static,
    ) -> Result<(), String> {
        let id = id.to_string();
        with_profile(app, move |profile, tx| unsafe {
            let reply = tx.clone();
            let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |result, list| {
                let found = result.and_then(|_| unsafe { all(list) }).map(|all| {
                    all.into_iter().find(|extension| unsafe { read(extension) }.is_ok_and(|f| f.id == id))
                });
                match found {
                    Ok(Some(extension)) => then(extension, reply),
                    Ok(None) => {
                        let _ = reply.send(Err("Uzantı bulunamadı".into()));
                    }
                    Err(error) => {
                        let _ = reply.send(Err(text(error)));
                    }
                }
                Ok(())
            }));
            if let Err(error) = profile.GetBrowserExtensions(&handler) {
                let _ = tx.send(Err(text(error)));
            }
        })
    }

    pub fn set_enabled(app: &AppHandle, id: &str, enabled: bool) -> Result<(), String> {
        with_extension(app, id, move |extension, tx| unsafe {
            let reply = tx.clone();
            let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(move |result| {
                let _ = reply.send(result.map_err(text));
                Ok(())
            }));
            if let Err(error) = extension.Enable(enabled, &handler) {
                let _ = tx.send(Err(text(error)));
            }
        })
    }

    pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
        with_extension(app, id, |extension, tx| unsafe {
            let reply = tx.clone();
            let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |result| {
                let _ = reply.send(result.map_err(text));
                Ok(())
            }));
            if let Err(error) = extension.Remove(&handler) {
                let _ = tx.send(Err(text(error)));
            }
        })
    }
}

/// Other systems: WebView2 is Windows only, so no extensions yet.
#[cfg(not(windows))]
mod native {
    use std::path::Path;

    use tauri::AppHandle;

    use super::Found;

    const NOT_HERE: &str = "Uzantılar şimdilik sadece Windows'ta çalışıyor";

    pub fn list(_app: &AppHandle) -> Result<Vec<Found>, String> {
        Ok(Vec::new())
    }
    pub fn add(_app: &AppHandle, _folder: &Path) -> Result<Found, String> {
        Err(NOT_HERE.into())
    }
    pub fn set_enabled(_app: &AppHandle, _id: &str, _enabled: bool) -> Result<(), String> {
        Err(NOT_HERE.into())
    }
    pub fn remove(_app: &AppHandle, _id: &str) -> Result<(), String> {
        Err(NOT_HERE.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_store_ids() {
        assert_eq!(
            store_id("https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh?hl=tr").as_deref(),
            Some(ADBLOCK_ID)
        );
        assert_eq!(store_id(" ddkjiahejlhfcafbddmgiahcphecmpfh ").as_deref(), Some(ADBLOCK_ID));
        assert_eq!(store_id("https://example.com/"), None);
    }

    #[test]
    fn reads_crx3_files() {
        let mut crx = b"Cr24".to_vec();
        crx.extend(3u32.to_le_bytes());
        crx.extend(5u32.to_le_bytes());
        crx.extend(b"head!PKrest");
        assert_eq!(zip_part(&crx).unwrap(), b"PKrest");
        assert!(zip_part(b"<html>").is_err());
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
