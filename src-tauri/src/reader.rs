//! Reading view: the article of the page, without menus, ads and sidebars, in
//! a calm layout. Pressing it again shows the page as it was.

use tauri::AppHandle;

use crate::tabs;

/// Builds the reading view over the page, or removes it if it is open.
const READER_SCRIPT: &str = r#"(() => {
  const old = document.getElementById("raven-reader");
  if (old) { old.remove(); document.documentElement.style.overflow = ""; return; }

  // The element with the most paragraph text is the article.
  const candidates = [...document.querySelectorAll("article, main, [role=main], div, section")];
  let best = null, bestScore = 0;
  for (const el of candidates) {
    const paragraphs = el.querySelectorAll(":scope > p, :scope > div > p, :scope > section > p");
    let score = 0;
    for (const p of paragraphs) score += Math.min(p.innerText.length, 600);
    if (el.tagName === "ARTICLE") score *= 1.4;
    if (score > bestScore) { best = el; bestScore = score; }
  }
  if (!best || bestScore < 400) best = document.querySelector("article, main") || document.body;

  const content = best.cloneNode(true);
  content.querySelectorAll("script, style, noscript, nav, aside, footer, form, iframe, button, input, select, [role=navigation], [aria-hidden=true], .ad, .ads, .advert, .share, .social, .related, .comments").forEach((n) => n.remove());
  content.querySelectorAll("*").forEach((n) => {
    n.removeAttribute("style");
    n.removeAttribute("class");
    n.removeAttribute("id");
    if (n.tagName === "IMG") {
      const src = n.getAttribute("data-src") || n.getAttribute("data-lazy-src");
      if (src) n.setAttribute("src", src);
      n.removeAttribute("width");
      n.removeAttribute("height");
    }
  });

  const minutes = Math.max(1, Math.round(content.innerText.split(/\s+/).length / 220));
  const root = document.createElement("div");
  root.id = "raven-reader";
  root.attachShadow({ mode: "open" }).innerHTML = `
    <style>
      :host { all: initial; position: fixed; inset: 0; z-index: 2147483647; overflow: auto; background: #f7f5ef; color: #1d1b26; }
      :host([data-dark]) { background: #15141f; color: #e9e7f2; }
      .bar { position: sticky; top: 0; display: flex; gap: 8px; justify-content: flex-end; padding: 12px 16px; background: inherit; }
      button { height: 34px; min-width: 34px; padding: 0 12px; border: 0; border-radius: 10px; background: rgba(127,127,160,.15); color: inherit; font: 700 13px system-ui, sans-serif; cursor: pointer; }
      article { max-width: 680px; margin: 0 auto; padding: 8px 28px 120px; font: 400 var(--size, 20px)/1.7 Georgia, "Iowan Old Style", "Times New Roman", serif; }
      h1 { font: 800 2.1em/1.15 system-ui, "Segoe UI", sans-serif; letter-spacing: -0.02em; margin: 0 0 .3em; }
      .meta { font: 600 14px system-ui, sans-serif; opacity: .6; margin-bottom: 2em; }
      h2, h3, h4 { font-family: system-ui, "Segoe UI", sans-serif; line-height: 1.25; margin-top: 1.6em; }
      img, video, figure { max-width: 100%; height: auto; border-radius: 10px; }
      a { color: #7b5cf0; }
      pre, code { font-family: Consolas, monospace; font-size: .85em; white-space: pre-wrap; }
      blockquote { margin: 1.2em 0; padding-left: 1em; border-left: 3px solid #a184f7; opacity: .85; }
    </style>
    <div class="bar">
      <button data-act="smaller" title="Yazıyı küçült">A−</button>
      <button data-act="bigger" title="Yazıyı büyüt">A+</button>
      <button data-act="theme" title="Açık / koyu">◐</button>
      <button data-act="close" title="Okuma görünümünü kapat">Kapat</button>
    </div>
    <article><h1></h1><div class="meta"></div><div class="body"></div></article>`;
  const shadow = root.shadowRoot;
  shadow.querySelector("h1").textContent = document.title;
  shadow.querySelector(".meta").textContent = location.hostname.replace(/^www\./, "") + " · " + minutes + " dk okuma";
  shadow.querySelector(".body").append(...content.childNodes);
  let size = 20;
  if (matchMedia("(prefers-color-scheme: dark)").matches) root.setAttribute("data-dark", "");
  shadow.querySelector(".bar").addEventListener("click", (event) => {
    const act = event.target.closest("button")?.dataset.act;
    if (act === "bigger") size = Math.min(30, size + 2);
    if (act === "smaller") size = Math.max(14, size - 2);
    if (act === "theme") root.toggleAttribute("data-dark");
    if (act === "close") { root.remove(); document.documentElement.style.overflow = ""; return; }
    shadow.querySelector("article").style.setProperty("--size", size + "px");
  });
  document.documentElement.style.overflow = "hidden";
  document.documentElement.appendChild(root);
})();"#;

/// Opens or closes the reading view in a window's active tab.
pub fn toggle(app: &AppHandle, window: &str) -> Result<(), String> {
    let tab = tabs::active_webview(app, window).ok_or("No page")?;
    tab.eval(READER_SCRIPT).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_reader(app: AppHandle, webview: tauri::Webview) -> Result<(), String> {
    toggle(&app, &crate::windows::window_of(&webview))
}
