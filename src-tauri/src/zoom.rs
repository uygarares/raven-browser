//! Page zoom, remembered per website in zoom.json, like Chrome.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Url};

use crate::storage;

const FILE: &str = "zoom.json";

/// The zoom steps Chrome uses.
pub const LEVELS: [f64; 17] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

#[derive(Default)]
pub struct ZoomLevels(Mutex<HashMap<String, f64>>);

pub fn load(app: &AppHandle) -> ZoomLevels {
    ZoomLevels(Mutex::new(storage::read(app, FILE)))
}

/// The website a zoom level belongs to, for example "www.youtube.com".
pub fn site_of(url: &str) -> Option<String> {
    let url: Url = url.parse().ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.host_str().map(String::from)
}

/// The saved zoom for a page's website, 1.0 if none.
pub fn level_for(app: &AppHandle, url: &str) -> f64 {
    let Some(site) = site_of(url) else { return 1.0 };
    app.state::<ZoomLevels>().0.lock().unwrap().get(&site).copied().unwrap_or(1.0)
}

/// Remembers a website's zoom (100% is not stored).
pub fn remember(app: &AppHandle, url: &str, level: f64) {
    let Some(site) = site_of(url) else { return };
    let levels = {
        let state = app.state::<ZoomLevels>();
        let mut levels = state.0.lock().unwrap();
        if (level - 1.0).abs() < 0.001 {
            levels.remove(&site);
        } else {
            levels.insert(site, level);
        }
        levels.clone()
    };
    if let Err(error) = storage::write(app, FILE, &levels) {
        eprintln!("Could not save zoom levels: {error}");
    }
}

/// The next zoom step up (+1) or down (-1); 0 goes back to 100%.
pub fn step(current: f64, direction: i32) -> f64 {
    match direction {
        0 => 1.0,
        d if d > 0 => LEVELS.iter().copied().find(|l| *l > current + 0.001).unwrap_or(LEVELS[16]),
        _ => LEVELS.iter().rev().copied().find(|l| *l < current - 0.001).unwrap_or(LEVELS[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::{site_of, step};

    #[test]
    fn steps_like_chrome() {
        assert_eq!(step(1.0, 1), 1.1);
        assert_eq!(step(1.0, -1), 0.9);
        assert_eq!(step(1.25, 0), 1.0);
        assert_eq!(step(5.0, 1), 5.0);
        assert_eq!(step(0.25, -1), 0.25);
        // A level between steps goes to the next real step.
        assert_eq!(step(1.2, 1), 1.25);
    }

    #[test]
    fn zoom_belongs_to_the_website() {
        assert_eq!(site_of("https://www.youtube.com/watch?v=1").as_deref(), Some("www.youtube.com"));
        assert_eq!(site_of("tauri://localhost/newtab.html"), None);
    }
}
