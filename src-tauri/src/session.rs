//! Session restore: open tabs are saved to session.json whenever they change,
//! and reopened on the next start. Only the active tab loads right away; the
//! others wait asleep until you click them, so starting with many tabs is fast.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::storage;

const FILE: &str = "session.json";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SavedTab {
    /// Empty for Raven's new tab page.
    pub url: String,
    pub title: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    /// Position of the active tab in `tabs`.
    pub active: usize,
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
