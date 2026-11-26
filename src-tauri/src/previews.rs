//! Small pictures of tabs for hover cards, like Chrome. A picture is taken
//! when you leave a tab, scaled down to 320 px wide and kept in memory only.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Webview};

use crate::native;

#[derive(Default)]
pub struct Previews(Mutex<HashMap<u32, String>>);

const WIDTH: u32 = 320;

/// PNG bytes → a small JPEG as a data: address.
pub fn shrink(png: &[u8]) -> Option<String> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let height = (image.height() as u64 * WIDTH as u64 / image.width().max(1) as u64).max(1) as u32;
    let small = image.resize_exact(WIDTH, height.min(WIDTH), image::imageops::FilterType::Triangle).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut out), 72).encode_image(&small).ok()?;
    Some(format!("data:image/jpeg;base64,{}", crate::extensions::base64_of(&out)))
}

/// Takes a picture of a tab in the background (on leaving it).
pub fn capture(app: &AppHandle, webview: &Webview, id: u32) {
    let Ok(pending) = native::screenshot_start(webview) else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        let png = pending.recv_timeout(std::time::Duration::from_secs(10)).ok().and_then(|r| r.ok());
        if let Some(picture) = png.and_then(|png| shrink(&png)) {
            app.state::<Previews>().0.lock().unwrap().insert(id, picture);
        }
    });
}

pub fn forget(app: &AppHandle, id: u32) {
    app.state::<Previews>().0.lock().unwrap().remove(&id);
}

#[tauri::command]
pub async fn get_tab_preview(app: AppHandle, id: u32) -> Option<String> {
    app.state::<Previews>().0.lock().unwrap().get(&id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_pictures() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(1280, 720, image::Rgb([120, 80, 200]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let small = shrink(&png).unwrap();
        assert!(small.starts_with("data:image/jpeg;base64,"));
        assert!(small.len() < 20_000);
    }
}
