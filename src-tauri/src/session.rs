//! Session restore: open windows and their tabs are saved to session.json
//! whenever they change, and reopened on the next start. Only each window's
//! active tab loads right away; the others wait asleep until you click them.
//! Private windows are never saved.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::storage;

const FILE: &str = "session.json";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SavedTab {
    /// Empty for Raven's new tab page.
    pub url: String,
    pub title: String,
    pub pinned: bool,
    /// When the tab was last opened (seconds since 1970), for the archive.
    pub last_used: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SavedWindow {
    pub tabs: Vec<SavedTab>,
    /// Position of the active tab in `tabs`.
    pub active: usize,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Session {
    pub windows: Vec<SavedWindow>,
    /// Older Raven versions saved a single window here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<SavedTab>,
    #[serde(skip_serializing_if = "is_zero")]
    pub active: usize,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl Session {
    /// The saved windows, also from the older single-window format.
    pub fn saved_windows(&self) -> Vec<SavedWindow> {
        if !self.windows.is_empty() {
            return self.windows.clone();
        }
        if self.tabs.is_empty() {
            return Vec::new();
        }
        vec![SavedWindow { tabs: self.tabs.clone(), active: self.active }]
    }
}

/// The last session written, so unchanged sessions are not written again.
static LAST_SAVED: Mutex<Option<Session>> = Mutex::new(None);

pub fn load(app: &AppHandle) -> Session {
    let session: Session = storage::read(app, FILE);
    *LAST_SAVED.lock().unwrap() = Some(session.clone());
    session
}

pub fn save(app: &AppHandle, session: Session) {
    let mut last = LAST_SAVED.lock().unwrap();
    if last.as_ref() == Some(&session) {
        return;
    }
    if let Err(error) = storage::write(app, FILE, &session) {
        eprintln!("Could not save the session: {error}");
        return;
    }
    *last = Some(session);
}

#[cfg(test)]
mod tests {
    use super::Session;

    #[test]
    fn reads_the_old_single_window_format() {
        let old = r#"{"tabs":[{"url":"https://a.com/","title":"A"},{"url":"","title":"Yeni sekme"}],"active":1}"#;
        let session: Session = serde_json::from_str(old).unwrap();
        let windows = session.saved_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].tabs.len(), 2);
        assert_eq!(windows[0].active, 1);
        assert!(!windows[0].tabs[0].pinned);
    }
}
