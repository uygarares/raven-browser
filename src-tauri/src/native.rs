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

/// Memory of Raven's browser processes: the total, and per page address
/// (the renderer that shows that page), in bytes.
pub struct Memory {
    pub total: u64,
    pub pages: Vec<(String, u64)>,
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
    pub fn memory(_webview: &Webview, tx: Sender<Result<Memory, String>>) -> Result<(), String> {
        let _ = tx.send(Err("Only on Windows".into()));
        Ok(())
    }
}
