//! WebView2 features Tauri does not offer: running a script and getting its
//! result, password saving, form filling, site colour scheme, pages going full
//! screen, and memory use. Windows only; other systems get stand-ins.

use std::sync::mpsc;
use std::time::Duration;

use tauri::{AppHandle, Webview};

/// Runs JavaScript in a webview and waits (up to 5 s) for its result as JSON.
/// Call it off the main thread.
pub fn run_script(webview: &Webview, script: &str) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    imp::run_script(webview, script.to_string(), tx)?;
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "The page did not answer".to_string())?
}

/// Applies Raven's settings to one tab: saving passwords and filling forms.
pub fn configure(webview: &Webview, save_passwords: bool, autofill: bool) {
    imp::configure(webview, save_passwords, autofill);
}

/// "auto", "dark" or "light" for all websites (the browser profile).
pub fn set_color_scheme(webview: &Webview, scheme: &str) {
    imp::set_color_scheme(webview, scheme);
}

/// Calls `on_change(true/false)` when a page in this tab enters or leaves full
/// screen (a video's full screen button).
pub fn watch_fullscreen(app: &AppHandle, webview: &Webview, on_change: fn(&AppHandle, &Webview, bool)) {
    imp::watch_fullscreen(app, webview, on_change);
}

/// Calls `check(address, page address, kind)` for every request a tab makes
/// (images, scripts, frames' content…); `true` blocks it. Runs on the main
/// thread, so `check` must be quick.
pub fn watch_requests(webview: &Webview, check: impl Fn(&str, &str, &'static str) -> bool + Send + 'static) {
    imp::watch_requests(webview, check);
}

/// Raven's own items in a page's right-click menu.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum MenuChoice {
    /// Ask the AI about the selected text.
    AskAi(String),
    /// Search the web for the selected text (new tab).
    Search(String),
    /// Open a link in a new tab.
    OpenLink(String),
    /// Summarize the page with the AI.
    Summarize,
    /// Search for a picture with Google Lens.
    SearchImage(String),
}

/// Adds Raven's items to the right-click menu of a tab.
pub fn extend_context_menu(webview: &Webview, on_pick: std::sync::Arc<dyn Fn(MenuChoice) + Send + Sync>) {
    imp::extend_context_menu(webview, on_pick);
}

/// Short label text for a selection: the first 24 characters.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn short(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 24 {
        format!("{}…", text.chars().take(24).collect::<String>())
    } else {
        text
    }
}

/// Takes a PNG picture of what a tab shows. Call it off the main thread.
pub fn screenshot(webview: &Webview) -> Result<Vec<u8>, String> {
    screenshot_start(webview)?.recv_timeout(Duration::from_secs(10)).map_err(|_| "No answer".to_string())?
}

/// Starts taking the picture now (before the tab is hidden); the result comes later.
pub fn screenshot_start(webview: &Webview) -> Result<mpsc::Receiver<Result<Vec<u8>, String>>, String> {
    let (tx, rx) = mpsc::channel();
    imp::screenshot(webview, tx)?;
    Ok(rx)
}

/// Opens WebView2's "Save as" window for the page (Ctrl+S).
pub fn save_page(webview: &Webview) {
    imp::save_page(webview);
}

/// How many cookies a site has; with `delete`, removes them too. Call it off
/// the main thread.
pub fn site_cookies(webview: &Webview, url: &str, delete: bool) -> Result<usize, String> {
    let (tx, rx) = mpsc::channel();
    imp::site_cookies(webview, url.to_string(), delete, tx)?;
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "No answer".to_string())?
}

/// Memory of Raven's browser processes: the total, and per page address
/// (the renderer that shows that page), in bytes.
pub struct Memory {
    pub total: u64,
    pub pages: Vec<(String, u64)>,
}

/// Runs a script as if the person had clicked (needed for picture in picture
/// and similar). Does not wait for an answer.
pub fn run_as_user(webview: &Webview, script: &str) {
    imp::run_as_user(webview, script.to_string());
}

/// Calls `on_failed(url, problem)` when a page fails to load: "404" or "410"
/// from the site, or "offline" when the site could not be reached.
pub fn watch_failures(webview: &Webview, on_failed: impl Fn(String, &'static str) + Send + 'static) {
    imp::watch_failures(webview, on_failed);
}

/// Measures memory (Windows). Call it off the main thread.
pub fn memory(webview: &Webview) -> Result<Memory, String> {
    let (tx, rx) = mpsc::channel();
    imp::memory(webview, tx)?;
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "No answer".to_string())?
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::Sender;

    use ::windows::core::{Interface, BOOL, HSTRING, PWSTR};
    use ::windows::Win32::Foundation::CloseHandle;
    use ::windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use ::windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use tauri::{AppHandle, Manager, Webview};
    use webview2_com::{
        ContainsFullScreenElementChangedEventHandler, CoTaskMemPWSTR, ExecuteScriptCompletedHandler,
        GetProcessExtendedInfosCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    };

    use super::Memory;

    pub fn run_script(webview: &Webview, script: String, tx: Sender<Result<String, String>>) -> Result<(), String> {
        webview
            .with_webview(move |platform| unsafe {
                let reply = tx.clone();
                let handler = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
                    let _ = reply.send(result.map(|_| json).map_err(|e| e.to_string()));
                    Ok(())
                }));
                let started = platform
                    .controller()
                    .CoreWebView2()
                    .and_then(|core| core.ExecuteScript(&HSTRING::from(script.as_str()), &handler));
                if let Err(error) = started {
                    let _ = tx.send(Err(error.to_string()));
                }
            })
            .map_err(|e| e.to_string())
    }

    pub fn configure(webview: &Webview, save_passwords: bool, autofill: bool) {
        let _ = webview.with_webview(move |platform| unsafe {
            let settings = platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings4>());
            if let Ok(settings) = settings {
                let _ = settings.SetIsPasswordAutosaveEnabled(save_passwords);
                let _ = settings.SetIsGeneralAutofillEnabled(autofill);
            }
        });
    }

    pub fn set_color_scheme(webview: &Webview, scheme: &str) {
        let value = match scheme {
            "dark" => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            "light" => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
            _ => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
        };
        let _ = webview.with_webview(move |platform| unsafe {
            let profile = platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile());
            if let Ok(profile) = profile {
                let _ = profile.SetPreferredColorScheme(value);
            }
        });
    }

    pub fn watch_fullscreen(app: &AppHandle, webview: &Webview, on_change: fn(&AppHandle, &Webview, bool)) {
        let app = app.clone();
        let label = webview.label().to_string();
        let _ = webview.with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else { return };
            let handler = ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    let mut full = BOOL::default();
                    if unsafe { sender.ContainsFullScreenElement(&mut full) }.is_ok() {
                        if let Some(webview) = app.get_webview(&label) {
                            on_change(&app, &webview, full.as_bool());
                        }
                    }
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = core.add_ContainsFullScreenElementChanged(&handler, &mut token);
        });
    }

    pub fn watch_requests(webview: &Webview, check: impl Fn(&str, &str, &'static str) -> bool + Send + 'static) {
        use webview2_com::WebResourceRequestedEventHandler;
        let _ = webview.with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else { return };
            let environment = platform.environment();
            let filter = HSTRING::from("*");
            let added = core
                .cast::<ICoreWebView2_22>()
                .and_then(|core| {
                    core.AddWebResourceRequestedFilterWithRequestSourceKinds(
                        &filter,
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                    )
                })
                .or_else(|_| core.AddWebResourceRequestedFilter(&filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL));
            if added.is_err() {
                return;
            }
            let handler = WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                unsafe {
                    // Global Privacy Control: asks every site not to sell or share your data.
                    if let Ok(headers) = args.Request().and_then(|r| r.Headers()) {
                        let _ = headers.SetHeader(&HSTRING::from("Sec-GPC"), &HSTRING::from("1"));
                    }
                    let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                    args.ResourceContext(&mut context)?;
                    let kind = match context {
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => return Ok(()),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
                        _ => "other",
                    };
                    let mut uri = PWSTR::null();
                    args.Request()?.Uri(&mut uri)?;
                    let uri = CoTaskMemPWSTR::from(uri).to_string();
                    if !(uri.starts_with("http://") || uri.starts_with("https://")) {
                        return Ok(());
                    }
                    let mut source = PWSTR::null();
                    sender.Source(&mut source)?;
                    let source = CoTaskMemPWSTR::from(source).to_string();
                    if check(&uri, &source, kind) {
                        let response = environment.CreateWebResourceResponse(
                            None::<&::windows::Win32::System::Com::IStream>,
                            403,
                            &HSTRING::from("Blocked by Raven Shield"),
                            &HSTRING::from(""),
                        )?;
                        args.SetResponse(&response)?;
                    }
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = core.add_WebResourceRequested(&handler, &mut token);
        });
    }

    pub fn run_as_user(webview: &Webview, script: String) {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        let params = serde_json::json!({ "expression": script, "userGesture": true, "awaitPromise": false }).to_string();
        let _ = webview.with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else { return };
            let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
            let _ = core.CallDevToolsProtocolMethod(&HSTRING::from("Runtime.evaluate"), &HSTRING::from(params.as_str()), &handler);
        });
    }

    pub fn watch_failures(webview: &Webview, on_failed: impl Fn(String, &'static str) + Send + 'static) {
        use webview2_com::NavigationCompletedEventHandler;
        let _ = webview.with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else { return };
            let handler = NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                unsafe {
                    let mut ok = BOOL::default();
                    args.IsSuccess(&mut ok)?;
                    let problem = if ok.as_bool() {
                        let mut status = 0i32;
                        if let Ok(args) = args.cast::<ICoreWebView2NavigationCompletedEventArgs2>() {
                            args.HttpStatusCode(&mut status)?;
                        }
                        match status {
                            404 => "404",
                            410 => "410",
                            _ => return Ok(()),
                        }
                    } else {
                        let mut error = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                        args.WebErrorStatus(&mut error)?;
                        match error {
                            COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED
                            | COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT
                            | COREWEBVIEW2_WEB_ERROR_STATUS_SERVER_UNREACHABLE
                            | COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT
                            | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET
                            | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED => "offline",
                            _ => return Ok(()),
                        }
                    };
                    let mut source = PWSTR::null();
                    sender.Source(&mut source)?;
                    let url = CoTaskMemPWSTR::from(source).to_string();
                    if url.starts_with("http") {
                        on_failed(url, problem);
                    }
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = core.add_NavigationCompleted(&handler, &mut token);
        });
    }

    pub fn extend_context_menu(webview: &Webview, on_pick: std::sync::Arc<dyn Fn(super::MenuChoice) + Send + Sync>) {
        use super::MenuChoice;
        use webview2_com::{ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler};
        let _ = webview.with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_11>()) else { return };
            let Ok(environment) = platform.environment().cast::<ICoreWebView2Environment9>() else { return };
            let handler = ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                unsafe {
                    let target = args.ContextMenuTarget()?;
                    let menu = args.MenuItems()?;
                    let mut choices: Vec<(String, MenuChoice)> = Vec::new();
                    let mut has = BOOL::default();
                    target.HasLinkUri(&mut has)?;
                    if has.as_bool() {
                        let mut link = PWSTR::null();
                        target.LinkUri(&mut link)?;
                        choices.push(("Bağlantıyı yeni sekmede aç".into(), MenuChoice::OpenLink(CoTaskMemPWSTR::from(link).to_string())));
                    }
                    let mut kind = COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND::default();
                    target.Kind(&mut kind)?;
                    target.HasSourceUri(&mut has)?;
                    if kind == COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE && has.as_bool() {
                        let mut source = PWSTR::null();
                        target.SourceUri(&mut source)?;
                        let source = CoTaskMemPWSTR::from(source).to_string();
                        if source.starts_with("http") {
                            choices.push(("Görseli Google Lens ile ara".into(), MenuChoice::SearchImage(source)));
                        }
                    }
                    target.HasSelection(&mut has)?;
                    if has.as_bool() {
                        let mut text = PWSTR::null();
                        target.SelectionText(&mut text)?;
                        let text = CoTaskMemPWSTR::from(text).to_string();
                        let label = super::short(&text);
                        choices.push((format!("Yapay zekaya sor: “{label}”"), MenuChoice::AskAi(text.clone())));
                        choices.push((format!("Ara: “{label}”"), MenuChoice::Search(text)));
                    }
                    if choices.is_empty() {
                        choices.push(("Bu sayfayı yapay zekayla özetle".into(), MenuChoice::Summarize));
                    }
                    let mut index = 0u32;
                    for (label, choice) in choices {
                        let item = environment.CreateContextMenuItem(
                            &HSTRING::from(label.as_str()),
                            None::<&::windows::Win32::System::Com::IStream>,
                            COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND,
                        )?;
                        let pick = on_pick.clone();
                        let selected = CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                            pick(choice.clone());
                            Ok(())
                        }));
                        let mut token = 0i64;
                        item.add_CustomItemSelected(&selected, &mut token)?;
                        menu.InsertValueAtIndex(index, &item)?;
                        index += 1;
                    }
                    let separator = environment.CreateContextMenuItem(
                        &HSTRING::from(""),
                        None::<&::windows::Win32::System::Com::IStream>,
                        COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR,
                    )?;
                    menu.InsertValueAtIndex(index, &separator)?;
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = core.add_ContextMenuRequested(&handler, &mut token);
        });
    }

    pub fn screenshot(webview: &Webview, tx: Sender<Result<Vec<u8>, String>>) -> Result<(), String> {
        use ::windows::Win32::System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
        use ::windows::Win32::UI::Shell::SHCreateMemStream;
        use webview2_com::CapturePreviewCompletedHandler;
        webview
            .with_webview(move |platform| unsafe {
                let Some(stream) = SHCreateMemStream(None) else {
                    let _ = tx.send(Err("No memory stream".into()));
                    return;
                };
                let reply = tx.clone();
                let reader = stream.clone();
                let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                    let bytes = result.and_then(|_| unsafe {
                        let mut stat = STATSTG::default();
                        reader.Stat(&mut stat, STATFLAG_NONAME)?;
                        reader.Seek(0, STREAM_SEEK_SET, None)?;
                        let mut bytes = vec![0u8; stat.cbSize as usize];
                        let mut read = 0u32;
                        reader.Read(bytes.as_mut_ptr() as *mut _, bytes.len() as u32, Some(&mut read)).ok()?;
                        bytes.truncate(read as usize);
                        Ok(bytes)
                    });
                    let _ = reply.send(bytes.map_err(|e| e.to_string()));
                    Ok(())
                }));
                let started = platform.controller().CoreWebView2().and_then(|core| {
                    core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler)
                });
                if let Err(error) = started {
                    let _ = tx.send(Err(error.to_string()));
                }
            })
            .map_err(|e| e.to_string())
    }

    pub fn save_page(webview: &Webview) {
        use webview2_com::ShowSaveAsUICompletedHandler;
        let _ = webview.with_webview(|platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_25>()) else { return };
            let handler = ShowSaveAsUICompletedHandler::create(Box::new(|_, _| Ok(())));
            let _ = core.ShowSaveAsUI(&handler);
        });
    }

    pub fn site_cookies(webview: &Webview, url: String, delete: bool, tx: Sender<Result<usize, String>>) -> Result<(), String> {
        use webview2_com::GetCookiesCompletedHandler;
        webview
            .with_webview(move |platform| unsafe {
                let manager = platform
                    .controller()
                    .CoreWebView2()
                    .and_then(|core| core.cast::<ICoreWebView2_2>())
                    .and_then(|core| core.CookieManager());
                let manager = match manager {
                    Ok(manager) => manager,
                    Err(error) => {
                        let _ = tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let reply = tx.clone();
                let deleter = manager.clone();
                let handler = GetCookiesCompletedHandler::create(Box::new(move |result, list| {
                    let count = result.and_then(|_| unsafe {
                        let Some(list) = list else { return Ok(0) };
                        let mut count = 0u32;
                        list.Count(&mut count)?;
                        if delete {
                            for i in 0..count {
                                let cookie = list.GetValueAtIndex(i)?;
                                let _ = deleter.DeleteCookie(&cookie);
                            }
                        }
                        Ok(count as usize)
                    });
                    let _ = reply.send(count.map_err(|e| e.to_string()));
                    Ok(())
                }));
                if let Err(error) = manager.GetCookies(&HSTRING::from(url.as_str()), &handler) {
                    let _ = tx.send(Err(error.to_string()));
                }
            })
            .map_err(|e| e.to_string())
    }

    fn working_set(pid: u32) -> u64 {
        unsafe {
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return 0 };
            let mut counters = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            let ok = GetProcessMemoryInfo(process, &mut counters, counters.cb).is_ok();
            let _ = CloseHandle(process);
            if ok { counters.WorkingSetSize as u64 } else { 0 }
        }
    }

    unsafe fn read_extended(
        list: Option<ICoreWebView2ProcessExtendedInfoCollection>,
    ) -> ::windows::core::Result<Memory> {
        let mut memory = Memory { total: 0, pages: Vec::new() };
        let Some(list) = list else { return Ok(memory) };
        unsafe {
            let mut count = 0u32;
            list.Count(&mut count)?;
            for index in 0..count {
                let info = list.GetValueAtIndex(index)?;
                let process = info.ProcessInfo()?;
                let mut pid = 0i32;
                process.ProcessId(&mut pid)?;
                let bytes = working_set(pid as u32);
                memory.total += bytes;
                let frames = info.AssociatedFrameInfos()?.GetIterator()?;
                // The renderer's memory goes to the top-level pages it shows.
                let mut sources = Vec::new();
                let mut has = BOOL::default();
                frames.HasCurrent(&mut has)?;
                while has.as_bool() {
                    let frame = frames.GetCurrent()?;
                    let top = frame
                        .cast::<ICoreWebView2FrameInfo2>()
                        .ok()
                        .map(|frame| frame.ParentFrameInfo().is_err())
                        .unwrap_or(true);
                    if top {
                        let mut source = PWSTR::null();
                        if frame.Source(&mut source).is_ok() {
                            sources.push(CoTaskMemPWSTR::from(source).to_string());
                        }
                    }
                    frames.MoveNext(&mut has)?;
                }
                if !sources.is_empty() {
                    let share = bytes / sources.len() as u64;
                    memory.pages.extend(sources.into_iter().map(|url| (url, share)));
                }
            }
        }
        Ok(memory)
    }

    pub fn memory(webview: &Webview, tx: Sender<Result<Memory, String>>) -> Result<(), String> {
        webview
            .with_webview(move |platform| unsafe {
                let reply = tx.clone();
                let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |result, list| {
                    let memory = result.and_then(|_| unsafe { read_extended(list) });
                    let _ = reply.send(memory.map_err(|e| e.to_string()));
                    Ok(())
                }));
                let started = platform
                    .environment()
                    .cast::<ICoreWebView2Environment13>()
                    .and_then(|environment| environment.GetProcessExtendedInfos(&handler));
                if let Err(error) = started {
                    let _ = tx.send(Err(error.to_string()));
                }
            })
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::mpsc::Sender;

    use tauri::{AppHandle, Webview};

    use super::Memory;

    pub fn run_script(_webview: &Webview, _script: String, tx: Sender<Result<String, String>>) -> Result<(), String> {
        let _ = tx.send(Err("Only on Windows".into()));
        Ok(())
    }
    pub fn configure(_webview: &Webview, _save_passwords: bool, _autofill: bool) {}
    pub fn set_color_scheme(_webview: &Webview, _scheme: &str) {}
    pub fn watch_fullscreen(_app: &AppHandle, _webview: &Webview, _on_change: fn(&AppHandle, &Webview, bool)) {}
    pub fn watch_requests(_webview: &Webview, _check: impl Fn(&str, &str, &'static str) -> bool + Send + 'static) {}
    pub fn extend_context_menu(_webview: &Webview, _on_pick: std::sync::Arc<dyn Fn(super::MenuChoice) + Send + Sync>) {}
    pub fn screenshot(_webview: &Webview, tx: Sender<Result<Vec<u8>, String>>) -> Result<(), String> {
        let _ = tx.send(Err("Only on Windows".into()));
        Ok(())
    }
    pub fn save_page(_webview: &Webview) {}
    pub fn run_as_user(_webview: &Webview, _script: String) {}
    pub fn watch_failures(_webview: &Webview, _on_failed: impl Fn(String, &'static str) + Send + 'static) {}
    pub fn site_cookies(_webview: &Webview, _url: String, _delete: bool, tx: Sender<Result<usize, String>>) -> Result<(), String> {
        let _ = tx.send(Err("Only on Windows".into()));
        Ok(())
    }
    pub fn memory(_webview: &Webview, tx: Sender<Result<Memory, String>>) -> Result<(), String> {
        let _ = tx.send(Err("Only on Windows".into()));
        Ok(())
    }
}
