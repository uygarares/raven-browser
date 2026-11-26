<p align="center"><img src="assets/raven-logo.png" width="160" alt="Raven logo"></p>

# Raven Browser

A fast browser that uses less RAM than Chrome, blocks ads by itself and has an
AI at hand. Built with Rust, Tauri and WebView2 (Chromium) for Windows;
Android comes later.

> Status: early development (0.2). Windows builds come from GitHub Actions.

## What it does

**Browsing**
- Tabs with groups (coloured, named, foldable), pinning, drag to reorder,
  hover cards with memory use, vertical tabs, split view (two tabs side by side)
- Several windows and private windows, session restore
- Address bar suggestions from history and bookmarks with inline completion
- Find in page, zoom, print, screenshot, save page, view source, reading view
- Bookmarks bar and bookmarks page, history, downloads, archive
- Command palette (Ctrl+K) and tab search (Ctrl+Shift+A)
- Media button for tabs that play sound
- Chrome's keyboard shortcuts

**Privacy and speed**
- Raven Shield: built-in ad and tracker blocking with Brave's engine
  (adblock-rust) and the EasyList, EasyPrivacy and uBlock lists, per-site switch
- Tab sleeping, a soft RAM limit, automatic archive of old tabs, game mode
- Always-secure connections, autoplay blocking, site permissions
- Password saving and form filling (WebView2, encrypted by Windows)

**AI**
- Claude, ChatGPT or Gemini in a side panel with your own account (no API key)
- "Özetle", "Açıkla", "Çevir" send the page to the AI; right-click a selection
  to ask about it; "Sor" on the new tab page

**Extensions**
- Chrome extensions from the Chrome Web Store ("Raven'a ekle" on store pages)

**Look**
- Dark, light or system theme, five accent colours, Manrope type

## Building

Windows 10/11 with Node.js, Rust and the Visual Studio C++ build tools:

```
npm install
npm run dev      # run
npm run build    # installer in src-tauri/target/release/bundle/nsis
```

## Credits

- [adblock-rust](https://github.com/brave/adblock-rust) by Brave (MPL 2.0)
- Filter lists: [EasyList, EasyPrivacy](https://easylist.to) and
  [uBlock Origin's lists](https://github.com/uBlockOrigin/uAssets), downloaded at run time
- [Manrope](https://github.com/sharanda/manrope) font (SIL Open Font License, ui/fonts/LICENSE)
- Early design notes from [unslop](https://github.com/heydamir/unslop) (CC BY 4.0)

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Raven Browser is free to use, and its source code is open to read, modify
and contribute to. Modified versions may not be published separately or
under another name. See [LICENSE](LICENSE) and [TRADEMARK.md](TRADEMARK.md).

---

## Türkçe

Raven, Chrome'dan daha az RAM kullanan, reklamları kendisi engelleyen ve içinde
yapay zeka olan hızlı bir tarayıcıdır. Ücretsizdir, kodu herkese açıktır ve
herkes katkı verebilir. Değiştirilmiş sürümler ayrı ya da başka bir isimle
yayınlanamaz.

**Kısayollar:** Ctrl+T yeni sekme · Ctrl+N yeni pencere · Ctrl+Shift+N gizli
pencere · Ctrl+K komut paleti · Ctrl+Boşluk yapay zeka · Ctrl+F sayfada bul ·
Ctrl+Shift+A sekme ara · Ctrl+Shift+O yer imleri · Ctrl+Shift+S ekran görüntüsü ·
F11 tam ekran
