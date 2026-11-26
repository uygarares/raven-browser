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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Solid,
            glass_tint: 0.3,
            onboarded: false,
            restore_session: true,
            show_bookmarks_bar: true,
        }
    }
}

impl Settings {
    fn clamped(mut self) -> Self {
        self.glass_tint = self.glass_tint.clamp(0.05, 0.85);
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
    // New tab pages read the theme when they load, so reload the open ones.
    crate::tabs::reload_new_tab_pages(&app);
    Ok(settings)
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
    }

    #[test]
    fn tint_stays_in_range() {
        let settings = Settings { glass_tint: 2.0, ..Default::default() }.clamped();
        assert_eq!(settings.glass_tint, 0.85);
    }
}
