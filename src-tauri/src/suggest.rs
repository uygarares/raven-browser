//! Address bar suggestions from history and bookmarks, like Chrome's omnibox.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{bookmarks::Bookmarks, history};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    /// "history" or "bookmark".
    pub kind: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Suggestions {
    pub items: Vec<Suggestion>,
    /// What the typed text completes to, like "youtube.com" for "you"
    /// (the toolbar shows the rest selected). None when nothing fits.
    pub inline: Option<String>,
}

/// "https://www.youtube.com/watch?v=1" becomes "youtube.com/watch?v=1".
pub fn short_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    rest.strip_suffix('/').unwrap_or(rest).to_string()
}

struct Candidate {
    url: String,
    title: String,
    visits: usize,
    last: i64,
    bookmarked: bool,
}

fn score(c: &Candidate, query: &str, words: &[String]) -> Option<i64> {
    let short = short_url(&c.url).to_lowercase();
    let title = c.title.to_lowercase();
    if !words.iter().all(|w| short.contains(w) || title.contains(w)) {
        return None;
    }
    let mut points = (c.visits.min(50) as i64) * 10;
    if c.bookmarked {
        points += 200;
    }
    if short.starts_with(query) {
        points += 1000;
        // A home page beats deeper pages of the same site.
        if !short.contains('/') {
            points += 300;
        }
    } else if title.starts_with(query) {
        points += 300;
    }
    Some(points)
}

/// Ranks pages for what was typed. `history` is oldest first.
pub fn rank(
    query: &str,
    history: &[history::Entry],
    bookmarks: &[(String, String)],
    limit: usize,
) -> Suggestions {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Suggestions { items: Vec::new(), inline: None };
    }
    let words: Vec<String> = query.split_whitespace().map(String::from).collect();

    let mut pages: HashMap<String, Candidate> = HashMap::new();
    for entry in history {
        let page = pages.entry(entry.url.clone()).or_insert(Candidate {
            url: entry.url.clone(),
            title: String::new(),
            visits: 0,
            last: 0,
            bookmarked: false,
        });
        page.visits += 1;
        page.last = page.last.max(entry.time);
        if !entry.title.is_empty() {
            page.title = entry.title.clone();
        }
    }
    // Home pages of visited sites, so "you" can complete to "youtube.com".
    let mut homes: HashMap<String, (usize, i64)> = HashMap::new();
    for page in pages.values() {
        let Ok(url) = page.url.parse::<tauri::Url>() else { continue };
        let Some(host) = url.host_str() else { continue };
        let home = homes.entry(format!("{}://{}/", url.scheme(), host)).or_insert((0, 0));
        home.0 += page.visits;
        home.1 = home.1.max(page.last);
    }
    for (url, (visits, last)) in homes {
        let page = pages.entry(url.clone()).or_insert(Candidate {
            url,
            title: String::new(),
            visits: 0,
            last: 0,
            bookmarked: false,
        });
        // Count the whole site's visits for its home page.
        page.visits = page.visits.max(visits);
        page.last = page.last.max(last);
    }
    for (url, title) in bookmarks {
        let page = pages.entry(url.clone()).or_insert(Candidate {
            url: url.clone(),
            title: title.clone(),
            visits: 0,
            last: 0,
            bookmarked: false,
        });
        page.bookmarked = true;
        if page.title.is_empty() {
            page.title = title.clone();
        }
    }

    let mut ranked: Vec<(i64, Candidate)> = pages
        .into_values()
        .filter_map(|c| score(&c, &query, &words).map(|s| (s, c)))
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.last.cmp(&a.1.last)).then(a.1.url.cmp(&b.1.url)));

    // Complete inline only when the best page's address starts with the text.
    let inline = ranked.first().and_then(|(_, c)| {
        let short = short_url(&c.url);
        (!query.contains(' ') && short.to_lowercase().starts_with(&query) && short.len() > query.len())
            .then_some(short)
    });

    let items = ranked
        .into_iter()
        .take(limit)
        .map(|(_, c)| Suggestion {
            kind: if c.bookmarked { "bookmark" } else { "history" },
            title: if c.title.is_empty() { short_url(&c.url) } else { c.title },
            url: c.url,
        })
        .collect();
    Suggestions { items, inline }
}

/// Where the chosen search engine answers "what might you be typing".
/// All of them answer in the OpenSearch format: ["query", ["one", "two"]].
fn suggest_url(engine: &str, query: &str) -> Option<String> {
    let (base, key, extra): (&str, &str, &[(&str, &str)]) = match engine {
        "google" => ("https://suggestqueries.google.com/complete/search", "q", &[("client", "firefox"), ("hl", "tr"), ("ie", "utf-8"), ("oe", "utf-8")]),
        "duckduckgo" => ("https://duckduckgo.com/ac/", "q", &[("type", "list"), ("kl", "tr-tr")]),
        "bing" => ("https://api.bing.com/osjson.aspx", "query", &[("mkt", "tr-TR")]),
        "brave" => ("https://search.brave.com/api/suggest", "q", &[]),
        "yandex" => ("https://suggest.yandex.com.tr/suggest-ff.cgi", "part", &[("uil", "tr")]),
        _ => return None,
    };
    let mut pairs: Vec<(&str, &str)> = extra.to_vec();
    pairs.push((key, query));
    tauri::Url::parse_with_params(base, &pairs).ok().map(|u| u.to_string())
}

/// The suggestions in an OpenSearch answer.
fn parse_suggestions(bytes: &[u8], query: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else { return Vec::new() };
    let query = query.trim().to_lowercase();
    value
        .get(1)
        .and_then(|list| list.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty() && text.to_lowercase() != query)
                .take(4)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// What the search engine suggests for the typed text (not in private
/// windows, and not when that setting is off).
#[tauri::command]
pub async fn search_suggestions(app: AppHandle, webview: tauri::Webview, query: String) -> Vec<String> {
    let (on, engine) = {
        let state = app.state::<crate::settings::SettingsState>();
        let settings = state.0.lock().unwrap();
        (settings.search_suggestions, settings.search_engine.clone())
    };
    let query = query.trim().to_string();
    if !on || query.is_empty() || query.len() > 200 || crate::windows::is_private(&app, &crate::windows::window_of(&webview)) {
        return Vec::new();
    }
    // Addresses are not sent to the search engine.
    if query.contains("://") || (!query.contains(' ') && query.contains('.') && !query.ends_with('.')) {
        return Vec::new();
    }
    let Some(url) = suggest_url(&engine, &query) else { return Vec::new() };
    tauri::async_runtime::spawn_blocking(move || {
        crate::extensions::fetch_quick(&url).map(|bytes| parse_suggestions(&bytes, &query)).unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn get_suggestions(app: AppHandle, query: String) -> Suggestions {
    let bookmarks: Vec<(String, String)> = app
        .state::<Bookmarks>()
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|b| (b.url.clone(), b.title.clone()))
        .collect();
    history::with_entries(&app, |entries| rank(&query, entries, &bookmarks, 6))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(url: &str, title: &str, time: i64) -> history::Entry {
        history::Entry { url: url.into(), title: title.into(), time }
    }

    #[test]
    fn reads_opensearch_suggestions() {
        let answer = br#"["hava", ["hava durumu", "Hava", "hava durumu istanbul", "havalimani", "havas", "x"]]"#;
        assert_eq!(parse_suggestions(answer, "hava"), vec!["hava durumu", "hava durumu istanbul", "havalimani", "havas"]);
        assert!(parse_suggestions(b"<html>", "a").is_empty());
        assert!(suggest_url("google", "çay saati").unwrap().contains("q=%C3%A7ay+saati"));
    }

    #[test]
    fn completes_to_the_most_visited_site() {
        let history = vec![
            visit("https://www.youtube.com/watch?v=1", "Video", 1),
            visit("https://www.youtube.com/watch?v=2", "Video 2", 2),
            visit("https://www.yourdictionary.com/", "Dictionary", 3),
        ];
        let result = rank("you", &history, &[], 6);
        assert_eq!(result.inline.as_deref(), Some("youtube.com"));
        assert_eq!(result.items[0].url, "https://www.youtube.com/");
    }

    #[test]
    fn finds_pages_by_title_and_bookmarks() {
        let bookmarks = vec![("https://github.com/".to_string(), "GitHub".to_string())];
        let result = rank("git", &[], &bookmarks, 6);
        assert_eq!(result.items[0].kind, "bookmark");
        assert_eq!(result.inline.as_deref(), Some("github.com"));
        let result = rank("hub", &[], &bookmarks, 6);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.inline, None);
    }

    #[test]
    fn empty_query_gives_nothing() {
        assert!(rank("  ", &[visit("https://a.com/", "A", 1)], &[], 6).items.is_empty());
    }
}
