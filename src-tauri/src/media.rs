//! Sound in tabs: shows which tabs are playing sound, and mutes them.
//!
//! On Windows this uses WebView2 directly (ICoreWebView2_8: IsDocumentPlayingAudio,
//! IsMuted), the same as Edge's speaker icon on tabs.

use tauri::{AppHandle, Webview};

/// Starts watching a tab for sound.
#[cfg(windows)]
pub fn watch(app: &AppHandle, webview: &Webview, id: u32) {
    use webview2_com::{IsDocumentPlayingAudioChangedEventHandler, Microsoft::Web::WebView2::Win32::*};
    use ::windows::core::Interface;

    let app = app.clone();
    let _ = webview.with_webview(move |platform| unsafe {
        let Ok(core) = platform.controller().CoreWebView2() else { return };
        let Ok(core8) = core.cast::<ICoreWebView2_8>() else { return };
        let handler = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| unsafe {
            let Some(sender) = sender else { return Ok(()) };
            let core8 = sender.cast::<ICoreWebView2_8>()?;
            let mut playing = ::windows::core::BOOL::default();
            core8.IsDocumentPlayingAudio(&mut playing)?;
            crate::tabs::set_audible(&app, id, playing.as_bool());
            Ok(())
        }));
        let mut token = 0i64;
        let _ = core8.add_IsDocumentPlayingAudioChanged(&handler, &mut token);
    });
}

/// Mutes or unmutes everything a tab plays.
#[cfg(windows)]
pub fn set_muted(webview: &Webview, muted: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use ::windows::core::Interface;

    let _ = webview.with_webview(move |platform| unsafe {
        let Ok(core) = platform.controller().CoreWebView2() else { return };
        if let Ok(core8) = core.cast::<ICoreWebView2_8>() {
            let _ = core8.SetIsMuted(muted);
        }
    });
}

/// Other systems: no sound indicator yet.
#[cfg(not(windows))]
pub fn watch(_app: &AppHandle, _webview: &Webview, _id: u32) {}

/// Other systems: mutes the page's audio and video elements.
#[cfg(not(windows))]
pub fn set_muted(webview: &Webview, muted: bool) {
    let _ = webview.eval(format!(
        "document.querySelectorAll('audio,video').forEach(m => m.muted = {muted})"
    ));
}
