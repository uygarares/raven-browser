// Raven toolbar: tabs, navigation and window buttons.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const tabsEl = document.getElementById("tabs");
const template = document.getElementById("tab-template");
const address = document.getElementById("address");
const loading = document.getElementById("loading");

let state = { tabs: [], active: null };

function run(command, args) {
  return invoke(command, args).catch((error) => console.error(command, error));
}

// Window buttons
document.getElementById("minimize").addEventListener("click", () => appWindow.minimize());
document.getElementById("maximize").addEventListener("click", () => appWindow.toggleMaximize());
document.getElementById("close").addEventListener("click", () => appWindow.close());

// Navigation
document.getElementById("back").addEventListener("click", () => run("go_back"));
document.getElementById("forward").addEventListener("click", () => run("go_forward"));
document.getElementById("reload").addEventListener("click", () => run("reload"));

document.getElementById("address-form").addEventListener("submit", (event) => {
  event.preventDefault();
  run("navigate", { input: address.value });
  address.blur();
});
address.addEventListener("focus", () => address.select());

// Tabs
document.getElementById("new-tab").addEventListener("click", () => run("run_action", { action: "new-tab" }));

function closeTab(id) {
  run("close_tab", { id });
}

const SEARCH_ICON =
  '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>';


function render() {
  tabsEl.replaceChildren(
    ...state.tabs.map((tab) => {
      const el = template.content.firstElementChild.cloneNode(true);
      const isActive = tab.id === state.active;
      const isNewTabPage = tab.url === "";
      el.classList.toggle("active", isActive);
      el.classList.toggle("loading", tab.loading && !isNewTabPage);
      el.classList.toggle("new-tab-page", isNewTabPage);
      el.setAttribute("aria-selected", String(isActive));
      el.title = tab.title;
      el.querySelector(".tab-title").textContent = tab.title;
      const icon = el.querySelector(".tab-icon");
      if (isNewTabPage) {
        icon.innerHTML = SEARCH_ICON;
      } else if (!tab.loading) {
        RavenIcons.fill(icon, tab.url, tab.title);
      }
      el.addEventListener("mousedown", (event) => {
        // Middle click closes the tab, like other browsers.
        if (event.button === 1) {
          event.preventDefault();
          closeTab(tab.id);
        } else if (event.button === 0 && !event.target.closest(".tab-close")) {
          run("switch_tab", { id: tab.id });
        }
      });
      el.querySelector(".tab-close").addEventListener("click", () => closeTab(tab.id));
      return el;
    }),
  );

  const active = state.tabs.find((tab) => tab.id === state.active);
  if (active) {
    if (document.activeElement !== address) {
      address.value = active.url;
    }
    loading.classList.toggle("on", active.loading && active.url !== "");
  }
  renderStar();
}

listen("tabs", ({ payload }) => {
  state = payload;
  render();
});

invoke("get_tabs").then((payload) => {
  state = payload;
  render();
});

// Bookmarks

const star = document.getElementById("star");
const bar = document.getElementById("bookmarks-bar");
let bookmarks = [];

function activeTab() {
  return state.tabs.find((tab) => tab.id === state.active);
}

function renderStar() {
  const tab = activeTab();
  const url = tab ? tab.url : "";
  star.hidden = url === "";
  const saved = bookmarks.some((bookmark) => bookmark.url === url);
  star.setAttribute("aria-pressed", String(saved));
  const label = saved ? "Yer iminden kaldır (Ctrl + D)" : "Yer imlerine ekle (Ctrl + D)";
  star.setAttribute("aria-label", label);
  star.title = label;
}


function renderBookmarks() {
  bar.replaceChildren(
    ...bookmarks.map((bookmark) => {
      const button = document.createElement("button");
      button.className = "bookmark";
      button.title = `${bookmark.title}\n${bookmark.url}\nSağ tık: kaldır`;
      const mark = document.createElement("span");
      mark.className = "mark-small";
      mark.setAttribute("aria-hidden", "true");
      RavenIcons.fill(mark, bookmark.url, bookmark.title);
      const title = document.createElement("span");
      title.className = "bookmark-title";
      title.textContent = bookmark.title;
      button.append(mark, title);
      button.addEventListener("click", (event) => {
        closePanel();
        run("open_url", { url: bookmark.url, newTab: event.ctrlKey });
      });
      button.addEventListener("auxclick", (event) => {
        if (event.button === 1) run("open_url", { url: bookmark.url, newTab: true });
      });
      button.addEventListener("contextmenu", (event) => {
        event.preventDefault();
        run("remove_bookmark", { id: bookmark.id });
      });
      return button;
    }),
  );
  renderStar();
}

star.addEventListener("click", () => run("toggle_bookmark"));

listen("bookmarks", ({ payload }) => {
  bookmarks = payload;
  renderBookmarks();
});
invoke("get_bookmarks").then((payload) => {
  bookmarks = payload;
  renderBookmarks();
});

function showBookmarksBar(show) {
  document.body.classList.toggle("with-bookmarks-bar", show);
}
listen("bookmarks-bar", ({ payload }) => showBookmarksBar(payload));
invoke("get_bookmarks_bar").then(showBookmarksBar);

// Most visited sites, for the new tab page's shortcuts until the user picks
// their own. The new tab page cannot ask Raven, so it reads this copy.
async function refreshTopSites() {
  const sites = await run("get_top_sites");
  if (!sites) return;
  try {
    localStorage.setItem("raven-top-sites", JSON.stringify(sites));
  } catch (error) {
    console.error(error);
  }
}
refreshTopSites();
setInterval(refreshTopSites, 30000);

// History panel

const historyPanel = document.getElementById("history-panel");
const historyList = document.getElementById("history-list");
const historyEmpty = document.getElementById("history-empty");
const historySearch = document.getElementById("history-search");
const historyClear = document.getElementById("history-clear");

const dayFormat = new Intl.DateTimeFormat("tr-TR", { weekday: "long", day: "numeric", month: "long" });
const clockFormat = new Intl.DateTimeFormat("tr-TR", { hour: "2-digit", minute: "2-digit" });

function dayLabel(date) {
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (date.toDateString() === today.toDateString()) return "Bugün";
  if (date.toDateString() === yesterday.toDateString()) return "Dün";
  return dayFormat.format(date);
}

function hostOf(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

async function loadHistory() {
  const entries = (await run("get_history", { query: historySearch.value })) || [];
  historyEmpty.hidden = entries.length > 0;
  historyEmpty.textContent = historySearch.value ? "Sonuç yok" : "Henüz geçmiş yok";
  const items = [];
  let lastDay = "";
  for (const entry of entries) {
    const date = new Date(entry.time);
    const day = dayLabel(date);
    if (day !== lastDay) {
      const heading = document.createElement("h2");
      heading.className = "history-day";
      heading.textContent = day;
      items.push(heading);
      lastDay = day;
    }
    const row = document.createElement("div");
    row.className = "history-row";

    const time = document.createElement("span");
    time.className = "history-time";
    time.textContent = clockFormat.format(date);

    const link = document.createElement("button");
    link.className = "history-link";
    link.title = entry.url;
    const mark = document.createElement("span");
    mark.className = "mark-small";
    mark.setAttribute("aria-hidden", "true");
    RavenIcons.fill(mark, entry.url, entry.title);
    const title = document.createElement("span");
    title.className = "history-title";
    title.textContent = entry.title || entry.url;
    const host = document.createElement("span");
    host.className = "history-host";
    host.textContent = hostOf(entry.url);
    link.append(mark, title, host);
    link.addEventListener("click", (event) => {
      closePanel();
      run("open_url", { url: entry.url, newTab: event.ctrlKey });
    });
    link.addEventListener("auxclick", (event) => {
      if (event.button === 1) run("open_url", { url: entry.url, newTab: true });
    });

    const remove = document.createElement("button");
    remove.className = "icon-button history-remove";
    remove.setAttribute("aria-label", "Geçmişten kaldır");
    remove.title = "Geçmişten kaldır";
    remove.innerHTML =
      '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
    remove.addEventListener("click", async () => {
      await run("remove_history_entry", { url: entry.url, time: entry.time });
      loadHistory();
    });

    row.append(time, link, remove);
    items.push(row);
  }
  historyList.replaceChildren(...items);
}

let searchTimer = null;
historySearch.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(loadHistory, 150);
});

historyClear.addEventListener("click", async () => {
  // Ask once more before deleting everything.
  if (historyClear.dataset.confirm !== "yes") {
    historyClear.dataset.confirm = "yes";
    historyClear.textContent = "Emin misin? Tekrar bas";
    setTimeout(() => {
      historyClear.dataset.confirm = "";
      historyClear.textContent = "Tümünü temizle";
    }, 3000);
    return;
  }
  historyClear.dataset.confirm = "";
  historyClear.textContent = "Tümünü temizle";
  await run("clear_history");
  loadHistory();
});

// Full-window panels: history, downloads and settings. One is open at a time;
// the tab row and address bar stay usable above it.

const panels = {
  history: { el: historyPanel, button: document.getElementById("history"), load: loadHistory, focus: historySearch },
  downloads: { el: document.getElementById("downloads-panel"), button: document.getElementById("downloads"), load: loadDownloads },
  settings: { el: document.getElementById("settings-panel"), button: document.getElementById("settings"), load: loadSettings },
};
let openPanelName = null;

async function openPanel(name) {
  const panel = panels[name];
  if (openPanelName === name) return;
  const wasOpen = openPanelName !== null;
  for (const [other, item] of Object.entries(panels)) {
    item.el.hidden = other !== name;
    item.button.setAttribute("aria-pressed", String(other === name));
  }
  openPanelName = name;
  if (!wasOpen) await run("set_full_window", { open: true });
  await panel.load();
  if (panel.focus) panel.focus.focus();
}

function closePanel() {
  if (openPanelName === null) return;
  for (const item of Object.values(panels)) {
    item.el.hidden = true;
    item.button.setAttribute("aria-pressed", "false");
  }
  openPanelName = null;
  run("set_full_window", { open: false });
}

function togglePanel(name) {
  if (openPanelName === name) closePanel();
  else openPanel(name);
}

for (const [name, item] of Object.entries(panels)) {
  item.button.addEventListener("click", () => togglePanel(name));
}
document.querySelectorAll(".panel-close").forEach((button) => button.addEventListener("click", closePanel));
listen("open-history", () => openPanel("history"));
listen("open-downloads", () => openPanel("downloads"));

// Using the tabs or the address bar closes the panel.
tabsEl.addEventListener("mousedown", closePanel);
document.getElementById("new-tab").addEventListener("click", closePanel);
document.getElementById("address-form").addEventListener("submit", closePanel);

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") closePanel();
});

// Downloads

const downloadsButton = document.getElementById("downloads");
const downloadsList = document.getElementById("downloads-list");
const downloadsEmpty = document.getElementById("downloads-empty");
let downloads = [];

const FILE_ICON =
  '<svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>';

function formatSize(bytes) {
  if (bytes == null) return "";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toLocaleString("tr-TR", { maximumFractionDigits: digits })} ${units[unit]}`;
}

function statusOf(download) {
  if (download.state === "in-progress") return "İndiriliyor";
  if (download.state === "failed") return "Başarısız";
  return formatSize(download.size) || "Tamamlandı";
}

function smallButton(label, onClick) {
  const button = document.createElement("button");
  button.className = "secondary small";
  button.textContent = label;
  button.addEventListener("click", onClick);
  return button;
}

function renderDownloads() {
  const busy = downloads.some((download) => download.state === "in-progress");
  downloadsButton.classList.toggle("busy", busy);
  downloadsEmpty.hidden = downloads.length > 0;
  downloadsList.replaceChildren(
    ...[...downloads].reverse().map((download) => {
      const row = document.createElement("div");
      row.className = `download-row ${download.state}`;

      const icon = document.createElement("span");
      icon.className = "download-icon";
      icon.innerHTML = FILE_ICON;

      const text = document.createElement("div");
      text.className = "download-text";
      const name = document.createElement("div");
      name.className = "download-name";
      name.textContent = download.fileName;
      name.title = download.path;
      const meta = document.createElement("div");
      meta.className = "download-meta";
      meta.textContent = `${statusOf(download)} · ${hostOf(download.url)}`;
      text.append(name, meta);

      const actions = document.createElement("div");
      actions.className = "download-actions";
      if (download.state === "done") {
        actions.append(
          smallButton("Aç", () => run("open_download", { id: download.id })),
          smallButton("Klasörde göster", () => run("show_download_in_folder", { id: download.id })),
        );
      }
      const remove = document.createElement("button");
      remove.className = "icon-button";
      remove.setAttribute("aria-label", "Listeden kaldır");
      remove.title = "Listeden kaldır";
      remove.innerHTML =
        '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
      remove.addEventListener("click", () => run("remove_download", { id: download.id }));
      actions.append(remove);

      row.append(icon, text, actions);
      return row;
    }),
  );
}

async function loadDownloads() {
  downloads = (await run("get_downloads")) || [];
  renderDownloads();
}

document.getElementById("downloads-clear").addEventListener("click", () => run("clear_downloads"));

listen("downloads", ({ payload }) => {
  downloads = payload;
  renderDownloads();
});
loadDownloads();

// Settings

let settings = null;
let engines = [];
const searchEngine = document.getElementById("search-engine");
const restoreSession = document.getElementById("restore-session");
const showBar = document.getElementById("show-bookmarks-bar");

/// The new tab page cannot ask Raven, so it reads the search address from here.
function rememberSearch() {
  const engine = engines.find((item) => item.id === settings?.searchEngine) || engines[0];
  if (!engine) return;
  try {
    localStorage.setItem("raven-search-url", JSON.stringify(engine.url));
  } catch (error) {
    console.error(error);
  }
}

function showSettings() {
  if (!settings) return;
  searchEngine.replaceChildren(
    ...engines.map((engine) => {
      const option = document.createElement("option");
      option.value = engine.id;
      option.textContent = engine.name;
      return option;
    }),
  );
  searchEngine.value = settings.searchEngine;
  restoreSession.checked = settings.restoreSession;
  showBar.checked = settings.showBookmarksBar;
}

async function loadSettings() {
  [settings, engines] = await Promise.all([run("get_settings"), run("get_search_engines")]);
  engines = engines || [];
  showSettings();
  rememberSearch();
}

async function saveSettings(change) {
  const saved = await run("save_settings", { settings: { ...settings, ...change } });
  if (saved) settings = saved;
  showSettings();
  rememberSearch();
}

searchEngine.addEventListener("change", () => saveSettings({ searchEngine: searchEngine.value }));
restoreSession.addEventListener("change", () => saveSettings({ restoreSession: restoreSession.checked }));
showBar.addEventListener("change", () => saveSettings({ showBookmarksBar: showBar.checked }));

listen("settings", ({ payload }) => {
  settings = payload;
  showSettings();
  rememberSearch();
});
// Ctrl + Shift + B changes the setting from Raven's side.
listen("bookmarks-bar", () => run("get_settings").then((saved) => {
  if (saved) {
    settings = saved;
    showSettings();
  }
}));

/// Buttons that ask once more before deleting something.
function confirmButton(button, action) {
  const label = button.textContent;
  button.addEventListener("click", async () => {
    if (button.dataset.confirm !== "yes") {
      button.dataset.confirm = "yes";
      button.textContent = "Emin misin? Tekrar bas";
      setTimeout(() => {
        button.dataset.confirm = "";
        button.textContent = label;
      }, 3000);
      return;
    }
    button.dataset.confirm = "";
    await action();
    button.textContent = "Temizlendi";
    setTimeout(() => (button.textContent = label), 2000);
  });
}

confirmButton(document.getElementById("settings-clear-history"), () => run("clear_history"));
confirmButton(document.getElementById("settings-clear-data"), () => run("clear_browsing_data"));
document.getElementById("settings-github").addEventListener("click", () => {
  closePanel();
  run("open_url", { url: "https://github.com/uygarares/raven-browser", newTab: true });
});

loadSettings();

// Keyboard shortcuts, the same as Chrome and Edge. Inside websites, Raven
// catches them itself (src-tauri/src/shortcuts.rs); these cover the toolbar.
function actionFor(event) {
  if (!event.ctrlKey || event.altKey) return null;
  const key = event.key.toLowerCase();
  const shift = event.shiftKey;
  if (key === "t") return shift ? "reopen-tab" : "new-tab";
  if (shift) {
    if (key === "tab") return "previous-tab";
    if (key === "b") return "toggle-bookmarks-bar";
    return null;
  }
  if (key === "h") return "open-history";
  if (key === "j") return "open-downloads";
  if (key === "d") return "toggle-bookmark";
  if (key === "w") return "close-tab";
  if (key === "l") return "focus-address";
  if (key === "tab" || key === "pagedown") return "next-tab";
  if (key === "pageup") return "previous-tab";
  if (key >= "1" && key <= "8") return { "go-to-tab": Number(key) - 1 };
  if (key === "9") return "last-tab";
  return null;
}

document.addEventListener("keydown", (event) => {
  const action = actionFor(event);
  if (!action) return;
  event.preventDefault();
  if (action === "focus-address") {
    closePanel();
    address.focus();
  } else if (action === "open-history") {
    togglePanel("history");
  } else if (action === "open-downloads") {
    togglePanel("downloads");
  } else {
    if (typeof action === "string" && action.endsWith("-tab")) closePanel();
    run("run_action", { action });
  }
});

listen("focus-address", () => {
  address.focus();
  address.select();
});
