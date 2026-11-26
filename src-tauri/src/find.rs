//! Find in page (Ctrl+F). Matches are marked with the CSS highlight API, so the
//! page itself is not changed; the current match is scrolled into view.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Webview};

use crate::{native, tabs, windows};

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Found {
    /// 1-based; 0 when nothing matches.
    pub current: u32,
    pub total: u32,
}

/// `step`: 0 searches again from the top, 1 next match, -1 previous.
fn script(query: &str, step: i32) -> String {
    let query = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const query = {query}, step = {step};
  const state = window.__ravenFind || (window.__ravenFind = {{ query: null, ranges: [], index: 0 }});
  if (!CSS.highlights) return JSON.stringify({{ current: 0, total: 0 }});
  if (!document.getElementById("raven-find-style")) {{
    const style = document.createElement("style");
    style.id = "raven-find-style";
    style.textContent = "::highlight(raven-find){{background:#ffe27a;color:#111}}::highlight(raven-find-now){{background:#ff9a3d;color:#111}}";
    (document.head || document.documentElement).appendChild(style);
  }}
  if (step === 0 || state.query !== query) {{
    state.query = query;
    state.ranges = [];
    state.index = 0;
    const needle = query.toLocaleLowerCase("tr");
    if (needle) {{
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT, {{
        acceptNode(node) {{
          const parent = node.parentElement;
          if (!parent || /^(SCRIPT|STYLE|NOSCRIPT|TEXTAREA)$/.test(parent.tagName)) return NodeFilter.FILTER_REJECT;
          return parent.offsetParent !== null || parent.getClientRects().length ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
        }},
      }});
      for (let node = walker.nextNode(); node && state.ranges.length < 2000; node = walker.nextNode()) {{
        const text = node.nodeValue.toLocaleLowerCase("tr");
        let at = text.indexOf(needle);
        while (at !== -1 && state.ranges.length < 2000) {{
          const range = new Range();
          range.setStart(node, at);
          range.setEnd(node, at + needle.length);
          state.ranges.push(range);
          at = text.indexOf(needle, at + needle.length);
        }}
      }}
    }}
  }} else if (state.ranges.length) {{
    state.index = (state.index + step + state.ranges.length) % state.ranges.length;
  }}
  CSS.highlights.set("raven-find", new Highlight(...state.ranges));
  const now = state.ranges[state.index];
  if (now) {{
    CSS.highlights.set("raven-find-now", new Highlight(now));
    const box = now.getBoundingClientRect();
    if (box.top < 80 || box.bottom > innerHeight - 40) {{
      window.scrollBy({{ top: box.top - innerHeight / 3, behavior: "smooth" }});
    }}
  }} else {{
    CSS.highlights.delete("raven-find-now");
  }}
  return JSON.stringify({{ current: state.ranges.length ? state.index + 1 : 0, total: state.ranges.length }});
}})()"#
    )
}

const CLEAR: &str = r#"(() => { if (CSS.highlights) { CSS.highlights.delete("raven-find"); CSS.highlights.delete("raven-find-now"); } window.__ravenFind = null; })()"#;

/// Parses the result of ExecuteScript: JSON of the JSON string the script returned.
pub fn parse(result: &str) -> Found {
    serde_json::from_str::<String>(result)
        .ok()
        .and_then(|inner| serde_json::from_str(&inner).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn find_in_page(app: AppHandle, webview: Webview, query: String, step: i32) -> Result<Found, String> {
    let window = windows::window_of(&webview);
    let tab = tabs::active_webview(&app, &window).ok_or("No page")?;
    let code = script(&query, step.clamp(-1, 1));
    tauri::async_runtime::spawn_blocking(move || native::run_script(&tab, &code).map(|r| parse(&r)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn stop_finding(app: AppHandle, webview: Webview) {
    if let Some(tab) = tabs::active_webview(&app, &windows::window_of(&webview)) {
        let _ = tab.eval(CLEAR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_script_results() {
        assert_eq!(parse(r#""{\"current\":2,\"total\":5}""#), Found { current: 2, total: 5 });
        assert_eq!(parse("null"), Found::default());
    }

    #[test]
    fn script_quotes_the_query() {
        assert!(script("a\"b", 1).contains(r#"const query = "a\"b", step = 1;"#));
    }
}
