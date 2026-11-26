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
        }
    }
}

impl Settings {
    fn clamped(mut self) -> Self {
        self.glass_tint = self.glass_tint.clamp(0.05, 0.85);
        if !SEARCH_ENGINES.iter().any(|(id, _, _)| *id == self.search_engine) {
            self.search_engine = "google".into();
        }
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
    store(&app, &settings)?;
    *app.state::<SettingsState>().0.lock().unwrap() = settings.clone();
    let _ = app.emit("settings", &settings);
    crate::bookmarks::refresh_bar(&app);
    // New tab pages read the search engine when they load, so reload the open ones.
    crate::tabs::reload_new_tab_pages(&app);
    Ok(settings)
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

/// Deletes cookies, cache and site data (you get signed out of websites).
#[tauri::command]
pub async fn clear_browsing_data(app: AppHandle) -> Result<(), String> {
    let webview = app
        .webviews()
        .into_iter()
        .find(|(label, _)| crate::tabs::is_tab_label(label))
        .map(|(_, webview)| webview)
        .or_else(|| app.get_webview("ui"))
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
