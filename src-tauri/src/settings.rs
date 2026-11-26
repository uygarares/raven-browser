//! Raven's settings, saved as settings.json in the app's config folder.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Flat dark surfaces.
    Solid,
    /// Reserved for themes from the theme store (not built yet).
    Glass,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    /// How dark the glass is, from 0.05 (almost clear) to 0.85 (almost solid).
    pub glass_tint: f32,
    /// False until the user picked a theme on the first launch.
    pub onboarded: bool,
    /// Reopen the last tabs when Raven starts.
    pub restore_session: bool,
    /// Show the bookmarks bar under the address bar (Ctrl+Shift+B).
    pub show_bookmarks_bar: bool,
    /// Search engine for the address bar and the new tab page (see SEARCH_ENGINES).
    pub search_engine: String,
    /// Raven's colours: "dark", "light" or "system".
    pub appearance: String,
    /// Colour of buttons and highlights: "violet", "teal", "blue", "rose", "amber".
    pub accent: String,
    /// Sites in "auto" (follow Raven), "dark" or "light".
    pub site_scheme: String,
    /// The AI in the side panel: "claude", "chatgpt" or "gemini".
    pub ai_provider: String,
    /// Offer to save passwords, and fill them in (WebView2's password manager).
    pub save_passwords: bool,
    /// Fill in addresses and forms.
    pub autofill: bool,
    /// Put tabs to sleep after this many minutes in the background (0: never).
    pub sleep_minutes: u32,
    /// Soft memory limit for all tabs in MB (0: no limit).
    pub memory_limit_mb: u32,
    /// Move tabs not opened for this many days to the archive (0: never).
    pub archive_days: u32,
    /// Game mode: put background tabs to sleep while a full screen game runs.
    pub game_mode: bool,
    /// Raven Shield blocks ads and trackers.
    pub shield: bool,
    /// The shield also hides "we use cookies" banners.
    pub hide_cookie_banners: bool,
    /// Videos start only after you click (muted autoplay still allowed).
    pub block_autoplay: bool,
    /// Upgrade http:// addresses to https://.
    pub https_upgrade: bool,
    /// Tabs in a list at the left instead of a row on top.
    pub vertical_tabs: bool,
    /// The welcome page version already seen (shown again when it changes).
    pub welcome: u32,
    /// Sites pinned to the side panel next to the AI (WhatsApp Web, Spotify…).
    pub panel_sites: Vec<PanelSite>,
    /// The theme from the theme store (its colours live in the toolbar).
    pub theme_id: String,
    /// Delete history, cookies and site data when Raven closes (like Brave).
    pub clear_on_exit: bool,
    /// Sync through the user's own Supabase project (address and public key).
    pub sync_url: String,
    pub sync_key: String,
    /// The encryption key made from the sync password (never the password),
    /// and the row id derived from it.
    pub sync_secret: String,
    pub sync_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PanelSite {
    pub name: String,
    pub url: String,
}

/// Search engines Raven offers: id, name, and the address the words are added to.
pub const SEARCH_ENGINES: &[(&str, &str, &str)] = &[
    ("google", "Google", "https://www.google.com/search?q="),
    ("duckduckgo", "DuckDuckGo", "https://duckduckgo.com/?q="),
    ("bing", "Bing", "https://www.bing.com/search?q="),
    ("brave", "Brave Search", "https://search.brave.com/search?q="),
    ("yandex", "Yandex", "https://yandex.com.tr/search/?text="),
];

/// The search address of the chosen engine.
pub fn search_url(app: &AppHandle) -> String {
    let id = app.state::<SettingsState>().0.lock().unwrap().search_engine.clone();
    SEARCH_ENGINES
        .iter()
        .find(|(engine, _, _)| *engine == id)
        .unwrap_or(&SEARCH_ENGINES[0])
        .2
        .to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Solid,
            glass_tint: 0.3,
            onboarded: false,
            restore_session: true,
            show_bookmarks_bar: true,
            search_engine: "google".into(),
            appearance: "dark".into(),
            accent: "violet".into(),
            site_scheme: "auto".into(),
            ai_provider: "claude".into(),
            save_passwords: true,
            autofill: true,
            sleep_minutes: 20,
            memory_limit_mb: 0,
            archive_days: 0,
            game_mode: true,
            shield: true,
            hide_cookie_banners: true,
            block_autoplay: false,
            https_upgrade: true,
            vertical_tabs: false,
            welcome: 0,
            panel_sites: Vec::new(),
            theme_id: "raven".into(),
            clear_on_exit: false,
            sync_url: String::new(),
            sync_key: String::new(),
            sync_secret: String::new(),
            sync_id: String::new(),
        }
    }
}

impl Settings {
    fn clamped(mut self) -> Self {
        self.glass_tint = self.glass_tint.clamp(0.05, 0.85);
        if !SEARCH_ENGINES.iter().any(|(id, _, _)| *id == self.search_engine) {
            self.search_engine = "google".into();
        }
        let pick = |value: &mut String, allowed: &[&str]| {
            if !allowed.contains(&value.as_str()) {
                *value = allowed[0].to_string();
            }
        };
        pick(&mut self.appearance, &["dark", "light", "system"]);
        pick(&mut self.accent, &["violet", "teal", "blue", "rose", "amber"]);
        pick(&mut self.site_scheme, &["auto", "dark", "light"]);
        pick(&mut self.ai_provider, &["claude", "chatgpt", "gemini"]);
        self.sleep_minutes = self.sleep_minutes.min(24 * 60);
        self
    }
}

pub struct SettingsState(pub Mutex<Settings>);

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("settings.json"))
}

/// Reads settings from disk, or the defaults if there are none yet.
pub fn load(app: &AppHandle) -> Settings {
    file(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default()
        .clamped()
}

pub fn store(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = file(app).ok_or("No settings folder")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Settings {
    app.state::<SettingsState>().0.lock().unwrap().clone()
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let settings = settings.clamped();
    let cookies_changed = app.state::<SettingsState>().0.lock().unwrap().hide_cookie_banners != settings.hide_cookie_banners;
    store(&app, &settings)?;
    *app.state::<SettingsState>().0.lock().unwrap() = settings.clone();
    let _ = app.emit("settings", &settings);
    crate::tabs::apply_settings(&app);
    crate::windows::set_vertical_tabs(&app, settings.vertical_tabs);
    crate::bookmarks::refresh_bar(&app);
    // New tab pages read the search engine when they load, so reload the open ones.
    crate::tabs::reload_new_tab_pages(&app);
    if cookies_changed {
        crate::shield::rebuild(&app);
    }
    Ok(settings)
}

/// Takes the settings from a backup.
pub fn apply_restored(app: &AppHandle, restored: Settings) -> Result<(), String> {
    let settings = Settings { welcome: restored.welcome.max(2), ..restored }.clamped();
    store(app, &settings)?;
    *app.state::<SettingsState>().0.lock().unwrap() = settings.clone();
    let _ = app.emit("settings", &settings);
    crate::bookmarks::refresh_bar(app);
    crate::tabs::apply_settings(app);
    crate::windows::set_vertical_tabs(app, settings.vertical_tabs);
    Ok(())
}

#[derive(Serialize)]
pub struct Engine {
    id: &'static str,
    name: &'static str,
    url: &'static str,
}

#[tauri::command]
pub async fn get_search_engines() -> Vec<Engine> {
    SEARCH_ENGINES.iter().map(|(id, name, url)| Engine { id, name, url }).collect()
}

/// "Tarama verilerini sil" (Ctrl+Shift+Delete): what, and since how many hours (0 = all).
#[tauri::command]
pub async fn clear_data(app: AppHandle, history: bool, kinds: crate::native::ClearKinds, hours: u32) -> Result<(), String> {
    let now = crate::tabs::now_secs();
    let since = if hours == 0 { 0 } else { now.saturating_sub(hours as u64 * 3600) };
    if history {
        crate::history::clear_since(&app, since as i64 * 1000);
    }
    let webview = normal_webview(&app)?;
    tauri::async_runtime::spawn_blocking(move || crate::native::clear_data(&webview, kinds, since as f64))
        .await
        .map_err(|e| e.to_string())?
}

/// A webview of a normal (not private) window: they all share the same browsing data.
fn normal_webview(app: &AppHandle) -> Result<tauri::Webview, String> {
    let normal = |label: &str| {
        let window = match label.strip_prefix("tab-") {
            Some(id) => id.parse().ok().and_then(|id| crate::tabs::window_of_tab(app, id)),
            None => label.strip_prefix("ui-").map(String::from),
        };
        window.is_some_and(|w| !crate::windows::is_private(app, &w))
    };
    app.webviews()
        .into_iter()
        .find(|(label, _)| label.starts_with("tab-") && normal(label))
        .or_else(|| app.webviews().into_iter().find(|(label, _)| label.starts_with("ui-") && normal(label)))
        .map(|(_, webview)| webview)
        .ok_or_else(|| "No webview".to_string())
}

/// Deletes cookies, cache and site data (you get signed out of websites).
#[tauri::command]
pub async fn clear_browsing_data(app: AppHandle) -> Result<(), String> {
    // Any webview of a normal (not private) window shares the same browsing data.
    let normal = |label: &str| {
        let window = match label.strip_prefix("tab-") {
            Some(id) => id.parse().ok().and_then(|id| crate::tabs::window_of_tab(&app, id)),
            None => label.strip_prefix("ui-").map(String::from),
        };
        window.is_some_and(|w| !crate::windows::is_private(&app, &w))
    };
    let webview = app
        .webviews()
        .into_iter()
        .find(|(label, _)| label.starts_with("tab-") && normal(label))
        .or_else(|| app.webviews().into_iter().find(|(label, _)| label.starts_with("ui-") && normal(label)))
        .map(|(_, webview)| webview)
        .ok_or("No webview")?;
    webview.clear_all_browsing_data().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Settings, Theme};

    #[test]
    fn missing_fields_use_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"glass"}"#).unwrap();
        assert_eq!(settings.theme, Theme::Glass);
        assert_eq!(settings.glass_tint, 0.3);
        assert!(!settings.onboarded);
        assert!(settings.restore_session);
        assert!(settings.show_bookmarks_bar);
        assert_eq!(settings.search_engine, "google");
    }

    #[test]
    fn tint_stays_in_range() {
        let settings = Settings { glass_tint: 2.0, ..Default::default() }.clamped();
        assert_eq!(settings.glass_tint, 0.85);
    }
}
