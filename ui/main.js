// Raven toolbar: tabs, navigation and window buttons. Every window has its own
// toolbar; events from Raven name the window they are for.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();
const WINDOW = appWindow.label;

const tabsEl = document.getElementById("tabs");
const template = document.getElementById("tab-template");
const address = document.getElementById("address");
const addressForm = document.getElementById("address-form");
const loading = document.getElementById("loading");
const zoomButton = document.getElementById("zoom");
const siteButton = document.getElementById("site-info");

let state = { tabs: [], active: null };

function run(command, args) {
  return invoke(command, args).catch((error) => console.error(command, error));
}

/// Listens to an event from Raven meant for this window.
function listenHere(name, handler) {
  listen(name, ({ payload }) => {
    const target = typeof payload === "string" ? payload : payload?.window;
    if (target === WINDOW) handler(payload);
  });
}

// Private window: tabs open in InPrivate mode and nothing is saved.
run("get_window_info").then((info) => {
  if (!info?.private) return;
  document.body.classList.add("private-window");
  document.getElementById("private-badge").hidden = false;
  document.title = "Raven (gizli)";
});

// Window buttons
document.getElementById("minimize").addEventListener("click", () => appWindow.minimize());
document.getElementById("maximize").addEventListener("click", () => appWindow.toggleMaximize());
document.getElementById("close").addEventListener("click", () => appWindow.close());

// Navigation
document.getElementById("back").addEventListener("click", () => run("go_back"));
document.getElementById("forward").addEventListener("click", () => run("go_forward"));
document.getElementById("reload").addEventListener("click", () => run("reload"));

addressForm.addEventListener("submit", (event) => {
  event.preventDefault();
  closeSuggestions();
  run("navigate", { input: address.value });
  address.blur();
});
address.addEventListener("focus", () => address.select());

// Popups: the ⋮ menu, the zoom box, the tab menu, site info and suggestions are
// drawn by popup.html in a small webview over the page.

let popupKind = null;

function openPopup(kind, data, { x, y, width, height, focus = true }) {
  popupKind = kind;
  return run("open_popup_command", { kind, data, x, y, width, height, focus });
}

function closePopup() {
  if (popupKind === null) return;
  popupKind = null;
  run("close_popup_command");
}

listenHere("popup-closed", () => {
  popupKind = null;
  suggestionsShown = false;
});

/// A button that opens a popup, and closes it when pressed again. Pressing it
/// takes the focus from the popup, which closes it first, so remember on
/// mousedown whether it was open.
function popupButton(button, kind, open) {
  let wasOpen = false;
  button.addEventListener("mousedown", () => (wasOpen = popupKind === kind));
  button.addEventListener("click", () => {
    if (wasOpen) {
      wasOpen = false;
      closePopup();
      return;
    }
    const rect = button.getBoundingClientRect();
    open(rect);
  });
}

function activeTab() {
  return state.tabs.find((tab) => tab.id === state.active);
}

// The ⋮ menu: new tab, new windows, zoom, history, downloads, bookmarks bar, settings.
const MENU_WIDTH = 328;
const MENU_HEIGHT = 17 * 34 + 4 * 11 + 14;
popupButton(document.getElementById("menu-button"), "menu", (rect) => {
  const tab = activeTab();
  const page = Boolean(tab && tab.url !== "");
  openPopup(
    "menu",
    { zoom: page ? tab.zoom : 1, page, bar: document.body.classList.contains("with-bookmarks-bar") },
    { x: rect.right - MENU_WIDTH, y: rect.bottom + 4, width: MENU_WIDTH, height: Math.min(MENU_HEIGHT, window.innerHeight - rect.bottom - 12) },
  );
});

// The zoom % in the address bar opens a box with − % + and Sıfırla, like Chrome.
popupButton(zoomButton, "zoom", (rect) => {
  const tab = activeTab();
  const width = 236;
  openPopup("zoom", { zoom: tab ? tab.zoom : 1 }, { x: rect.right - width + 20, y: rect.bottom + 8, width, height: 52 });
});

// The lock: is the connection secure, and the site's permissions.
popupButton(siteButton, "site-info", (rect) => {
  const tab = activeTab();
  if (!tab || tab.url === "") return;
  const web = /^https?:/i.test(tab.url);
  openPopup("site-info", { url: tab.url }, { x: rect.left - 6, y: rect.bottom + 8, width: 320, height: web ? 352 : 120 });
});

// Raven Shield: lit while it blocks, with the number blocked on this page.
const adblockButton = document.getElementById("adblock-button");
const shieldCount = document.createElement("span");
shieldCount.className = "shield-count";
shieldCount.hidden = true;
adblockButton.append(shieldCount);
let shield = null;
const pageCounts = new Map();

function showShieldCount() {
  const count = pageCounts.get(state.active) || 0;
  shieldCount.hidden = count === 0 || !shield?.on || !shield?.siteOn;
  shieldCount.textContent = count > 99 ? "99+" : String(count);
}

async function refreshShield() {
  shield = await run("get_shield");
  if (!shield) return;
  const lit = shield.on && shield.siteOn && shield.ready;
  adblockButton.classList.toggle("on", lit);
  adblockButton.classList.toggle("busy", shield.on && !shield.ready && !shield.error);
  const label = !shield.on ? "Raven Kalkanı kapalı" : !shield.siteOn ? "Bu sitede kalkan kapalı" : "Raven Kalkanı: reklamlar ve izleyiciler engelleniyor";
  adblockButton.title = label;
  adblockButton.setAttribute("aria-label", label);
  if (typeof shield.page === "number" && state.active != null) pageCounts.set(state.active, shield.page);
  showShieldCount();
}
refreshShield();
listen("shield-ready", refreshShield);
listen("settings", refreshShield);
listen("shield-counts", ({ payload }) => {
  for (const [id, count] of payload) pageCounts.set(id, count);
  showShieldCount();
});

const SHIELD_HEIGHT = 338;
popupButton(adblockButton, "adblock", async (rect) => {
  await refreshShield();
  const width = 320;
  openPopup("adblock", shield || {}, { x: rect.right - width, y: rect.bottom + 4, width, height: SHIELD_HEIGHT + (shield?.error ? 46 : 0) });
});

popupButton(document.getElementById("extensions-button"), "extensions", async (rect) => {
  let items = [];
  try {
    items = await invoke("get_extensions");
  } catch (error) {
    console.error(error);
  }
  const width = 320;
  const rows = Math.max(items.length, 1);
  openPopup("extensions", { items }, { x: rect.right - width, y: rect.bottom + 4, width, height: 40 + rows * 44 + 11 + 34 + 14 });
});

// Address bar suggestions from history and bookmarks.

let typed = ""; // What the user typed (without the completed part).
let rows = [];
let selected = -1; // -1: what is in the address bar.
let suggestionsShown = false;
let suggestToken = 0;

function closeSuggestions() {
  suggestToken += 1;
  rows = [];
  selected = -1;
  if (suggestionsShown) {
    suggestionsShown = false;
    closePopup();
  }
}

function showSuggestions() {
  if (rows.length === 0) {
    closeSuggestions();
    return;
  }
  const rect = addressForm.getBoundingClientRect();
  suggestionsShown = true;
  openPopup(
    "suggestions",
    { rows, selected },
    { x: rect.left, y: rect.bottom + 4, width: rect.width, height: rows.length * 36 + 14, focus: false },
  );
}

address.addEventListener("input", async (event) => {
  typed = address.value;
  selected = -1;
  const query = typed;
  const token = ++suggestToken;
  if (!query.trim()) {
    closeSuggestions();
    return;
  }
  const result = await run("get_suggestions", { query });
  if (token !== suggestToken || !result || document.activeElement !== address) return;
  // Complete the address inline, with the added part selected, unless the
  // user is deleting.
  const deleting = (event.inputType || "").startsWith("delete");
  const atEnd = address.selectionStart === query.length && address.selectionEnd === query.length;
  if (!deleting && atEnd && result.inline && address.value === query) {
    address.value = query + result.inline.slice(query.length);
    address.setSelectionRange(query.length, address.value.length);
  }
  rows = [{ kind: "search", text: query }, ...result.items];
  showSuggestions();
});

address.addEventListener("keydown", (event) => {
  if ((event.key === "ArrowDown" || event.key === "ArrowUp") && rows.length > 0) {
    event.preventDefault();
    const step = event.key === "ArrowDown" ? 1 : -1;
    // Cycle through -1 (what was typed), 0 … rows.length - 1.
    selected = ((selected + 1 + step + rows.length + 1) % (rows.length + 1)) - 1;
    const row = rows[selected];
    address.value = !row ? typed : row.kind === "search" ? row.text : row.url;
    run("popup_update", { data: { selected } });
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    if (suggestionsShown) {
      address.value = typed;
      closeSuggestions();
    } else {
      const tab = activeTab();
      address.value = tab ? tab.url : "";
      address.blur();
    }
  }
});

address.addEventListener("blur", () => {
  // Later, so a click on a suggestion is not lost.
  setTimeout(() => {
    if (document.activeElement !== address) closeSuggestions();
  }, 150);
});

// Tabs
document.getElementById("new-tab").addEventListener("click", () => run("run_action", { action: "new-tab" }));

function closeTab(id) {
  run("close_tab", { id });
}

const SEARCH_ICON =
  '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>';
const SOUND_ICON =
  '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9v6h4l5 4V5L8 9z"/><path d="M16.5 8.5a5 5 0 0 1 0 7"/></svg>';
const MUTED_ICON =
  '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9v6h4l5 4V5L8 9z"/><path d="M17 9l4 6M21 9l-4 6"/></svg>';

// Right click on a tab: the tab menu.
function openTabMenu(tab, event) {
  const index = state.tabs.findIndex((item) => item.id === tab.id);
  const others = state.tabs.some((item) => item.id !== tab.id && !item.pinned);
  const right = state.tabs.slice(index + 1).some((item) => !item.pinned);
  const height = 9 * 32 + 4 * 11 + 14;
  openPopup(
    "tab-menu",
    { id: tab.id, pinned: tab.pinned, muted: tab.muted, others, right },
    { x: event.clientX, y: event.clientY, width: 300, height },
  );
}

// Dragging tabs to change their order. Pointer events, because WebView2 does
// not pass HTML drag and drop through while Tauri listens for dropped files.
let drag = null; // { id, startX, offset, moved }
let renderLater = false;

function tabElement(id) {
  return tabsEl.querySelector(`.tab[data-id="${id}"]`);
}

function startDrag(tab, event) {
  drag = { id: tab.id, startX: event.clientX, startY: event.clientY, moved: false };
}

document.addEventListener("mousemove", (event) => {
  if (!drag) return;
  const vertical = document.body.classList.contains("vertical-tabs");
  const dx = vertical ? event.clientY - drag.startY : event.clientX - drag.startX;
  if (!drag.moved && Math.abs(dx) < 6) return;
  const el = tabElement(drag.id);
  if (!el) return;
  if (!drag.moved) {
    drag.moved = true;
    document.body.classList.add("dragging-tab");
    el.classList.add("dragged");
  }
  el.style.transform = vertical ? `translateY(${dx}px)` : `translateX(${dx}px)`;
});

document.addEventListener("mouseup", (event) => {
  if (!drag) return;
  const { id, moved } = drag;
  drag = null;
  document.body.classList.remove("dragging-tab");
  if (!moved) return;
  // The new place: how many other tabs have their middle before the pointer
  // (left of it, or above it with vertical tabs).
  const vertical = document.body.classList.contains("vertical-tabs");
  const others = [...tabsEl.querySelectorAll(".tab")].filter((el) => el.dataset.id !== String(id));
  const index = others.filter((el) => {
    const rect = el.getBoundingClientRect();
    return vertical ? rect.top + rect.height / 2 < event.clientY : rect.left + rect.width / 2 < event.clientX;
  }).length;
  const el = tabElement(id);
  if (el) {
    el.style.transform = "";
    el.classList.remove("dragged");
  }
  run("move_tab_command", { id, index });
  if (renderLater) render();
});

// Tab elements by id. They are updated in place, not drawn again, so switching
// and loading never make the tab row flicker.
const tabEls = new Map();

function makeTab(tab) {
  const el = template.content.firstElementChild.cloneNode(true);
  el.dataset.id = String(tab.id);
  el.classList.add("opening");
  el.addEventListener("animationend", () => el.classList.remove("opening"), { once: true });
  // Event handlers read the current tab from the state, not from when it was made.
  const current = () => state.tabs.find((item) => item.id === tab.id);
  el.querySelector(".tab-audio").addEventListener("click", (event) => {
    event.stopPropagation();
    const now = current();
    if (now) run("tab_action", { id: now.id, action: now.muted ? "unmute" : "mute" });
  });
  el.addEventListener("mousedown", (event) => {
    if (event.target.closest(".tab-close, .tab-audio")) return;
    // Middle click closes the tab, like other browsers.
    if (event.button === 1) {
      event.preventDefault();
      closeTab(tab.id);
    } else if (event.button === 0) {
      // Show it as active right away; Raven confirms a moment later.
      for (const other of tabEls.values()) other.classList.toggle("active", other === el);
      run("switch_tab", { id: tab.id });
      startDrag(tab, event);
    }
  });
  el.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const now = current();
    if (now) openTabMenu(now, event);
  });
  el.querySelector(".tab-close").addEventListener("click", () => closeTab(tab.id));
  return el;
}

function updateTab(el, tab) {
  const isActive = tab.id === state.active;
  const isNewTabPage = tab.url === "";
  const isLoading = tab.loading && !isNewTabPage;
  el.classList.toggle("active", isActive);
  el.classList.toggle("loading", isLoading);
  el.classList.toggle("new-tab-page", isNewTabPage);
  el.classList.toggle("pinned", Boolean(tab.pinned));
  el.classList.toggle("asleep", Boolean(tab.asleep) && !isActive);
  el.setAttribute("aria-selected", String(isActive));
  if (el.title !== tab.title) {
    el.title = tab.title;
    el.querySelector(".tab-title").textContent = tab.title;
  }
  // The icon changes only when the site or the loading state changes.
  const iconKey = isNewTabPage ? "new" : isLoading ? "loading" : RavenIcons.hostOf(tab.url);
  if (el.dataset.icon !== iconKey) {
    el.dataset.icon = iconKey;
    const icon = el.querySelector(".tab-icon");
    icon.className = "tab-icon";
    icon.replaceChildren();
    if (isNewTabPage) icon.innerHTML = SEARCH_ICON;
    else if (!isLoading) RavenIcons.fill(icon, tab.url, tab.title);
  }
  // Sound: a speaker on tabs that play sound; press it to mute the site.
  const audio = el.querySelector(".tab-audio");
  const audioKey = tab.muted ? "muted" : tab.audible ? "sound" : "";
  if (audio.dataset.state !== audioKey) {
    audio.dataset.state = audioKey;
    audio.hidden = audioKey === "";
    audio.innerHTML = tab.muted ? MUTED_ICON : SOUND_ICON;
    const label = tab.muted ? "Sitenin sesini aç" : "Siteyi sessize al";
    audio.setAttribute("aria-label", label);
    audio.title = label;
  }
}

function render() {
  // Moving elements would drop the tab being dragged.
  if (drag?.moved) {
    renderLater = true;
    return;
  }
  renderLater = false;
  const seen = new Set();
  state.tabs.forEach((tab, index) => {
    let el = tabEls.get(tab.id);
    if (!el) {
      el = makeTab(tab);
      tabEls.set(tab.id, el);
    }
    seen.add(tab.id);
    updateTab(el, tab);
    if (tabsEl.children[index] !== el) tabsEl.insertBefore(el, tabsEl.children[index] || null);
  });
  for (const [id, el] of tabEls) {
    if (!seen.has(id)) {
      el.remove();
      tabEls.delete(id);
    }
  }

  const active = activeTab();
  if (active) {
    if (document.activeElement !== address) {
      address.value = active.url;
    }
    loading.classList.toggle("on", active.loading && active.url !== "");
    const zoomed = active.url !== "" && Math.abs(active.zoom - 1) > 0.001;
    zoomButton.hidden = !zoomed;
    zoomButton.textContent = `${Math.round(active.zoom * 100)}%`;
    if (!zoomed && popupKind === "zoom") closePopup();
    renderSecurity(active.url);
  }
  renderStar();
}

// The icon at the start of the address bar: a lock on secure sites, a warning
// and "Güvenli değil" on http, a magnifier on the new tab page.
const SITE_ICONS = {
  search:
    '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>',
  lock: '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>',
  warning:
    '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4l9 16H3z"/><path d="M12 10v4M12 17v.5"/></svg>',
  info: '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8v.5"/></svg>',
};
let shownSecurity = null;

function renderSecurity(url) {
  const kind = url === "" ? "search" : /^https:/i.test(url) ? "lock" : /^http:/i.test(url) ? "warning" : "info";
  if (kind === shownSecurity) return;
  shownSecurity = kind;
  siteButton.innerHTML = SITE_ICONS[kind] + (kind === "warning" ? '<span class="not-secure">Güvenli değil</span>' : "");
  siteButton.disabled = kind === "search";
  siteButton.classList.toggle("unsafe", kind === "warning");
  const label = kind === "lock" ? "Bağlantı güvenli. Site bilgileri" : kind === "warning" ? "Bağlantı güvenli değil. Site bilgileri" : "Site bilgileri";
  siteButton.setAttribute("aria-label", label);
  siteButton.title = kind === "search" ? "" : label;
}

listenHere("tabs", (payload) => {
  const switched = payload.active !== state.active;
  state = payload;
  render();
  if (switched) showShieldCount();
});

run("get_tabs").then((payload) => {
  if (!payload) return;
  state = payload;
  render();
});

// Bookmarks

const star = document.getElementById("star");
const bar = document.getElementById("bookmarks-bar");
let bookmarks = [];

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
  history: { el: historyPanel, load: loadHistory, focus: historySearch },
  downloads: { el: document.getElementById("downloads-panel"), button: document.getElementById("downloads"), load: loadDownloads },
  settings: { el: document.getElementById("settings-panel"), load: () => { loadSettings(); showMemory(); loadImportSources(); showDefault(); } },
  archive: { el: document.getElementById("archive-panel"), load: loadArchive },
  bookmarks: { el: document.getElementById("bookmarks-panel"), load: loadBookmarksPage, focus: document.getElementById("bookmarks-search") },
  extensions: { el: document.getElementById("extensions-panel"), load: loadExtensions },
};
let openPanelName = null;

async function openPanel(name) {
  const panel = panels[name];
  if (openPanelName === name) return;
  const wasOpen = openPanelName !== null;
  for (const [other, item] of Object.entries(panels)) {
    item.el.hidden = other !== name;
    item.button?.setAttribute("aria-pressed", String(other === name));
  }
  openPanelName = name;
  document.body.classList.add("panel-open");
  if (!wasOpen) await run("set_full_window_command", { open: true });
  await panel.load();
  if (panel.focus) panel.focus.focus();
}

function closePanel() {
  if (openPanelName === null) return;
  for (const item of Object.values(panels)) {
    item.el.hidden = true;
    item.button?.setAttribute("aria-pressed", "false");
  }
  openPanelName = null;
  document.body.classList.remove("panel-open");
  run("set_full_window_command", { open: false });
}

function togglePanel(name) {
  if (openPanelName === name) closePanel();
  else openPanel(name);
}

for (const [name, item] of Object.entries(panels)) {
  item.button?.addEventListener("click", () => togglePanel(name));
}
document.querySelectorAll(".panel-close").forEach((button) => button.addEventListener("click", closePanel));
listenHere("open-history", () => openPanel("history"));
listenHere("open-downloads", () => openPanel("downloads"));
listenHere("open-settings", () => openPanel("settings"));
listenHere("open-archive", () => openPanel("archive"));
listenHere("open-extensions", (payload) => {
  openPanel("extensions");
  if (payload.install) askToInstall(payload.install);
});

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

let downloadedThisSession = false;

function renderDownloads() {
  const busy = downloads.some((download) => download.state === "in-progress");
  downloadsButton.classList.toggle("busy", busy);
  // Like Chrome, the downloads button appears once something is downloaded.
  downloadsButton.hidden = !(busy || downloadedThisSession);
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

listen("download-started", () => {
  downloadedThisSession = true;
});
listen("downloads", ({ payload }) => {
  downloads = payload;
  renderDownloads();
});
loadDownloads();

// Settings. Every input with data-setting="name" shows and saves that setting.

let settings = null;
let engines = [];
const searchEngine = document.getElementById("search-engine");
const systemDark = matchMedia("(prefers-color-scheme: dark)");

/// Raven's colours: dark, light or the system's, and the accent colour.
function applyLook() {
  if (!settings) return;
  const appearance = settings.appearance === "system" ? (systemDark.matches ? "dark" : "light") : settings.appearance;
  document.documentElement.dataset.appearance = appearance;
  document.documentElement.dataset.accent = settings.accent;
  document.body.classList.toggle("vertical-tabs", Boolean(settings.verticalTabs));
  // The new tab page and the menus read these.
  try {
    localStorage.setItem("raven-appearance", appearance);
    localStorage.setItem("raven-accent", settings.accent);
  } catch {}
}
systemDark.addEventListener("change", applyLook);

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
  if (searchEngine.options.length !== engines.length) {
    searchEngine.replaceChildren(
      ...engines.map((engine) => {
        const option = document.createElement("option");
        option.value = engine.id;
        option.textContent = engine.name;
        return option;
      }),
    );
  }
  for (const el of document.querySelectorAll("[data-setting]")) {
    const value = settings[el.dataset.setting];
    if (el.type === "checkbox") el.checked = Boolean(value);
    else if (el.tagName === "SELECT") el.value = String(value);
    else {
      for (const option of el.querySelectorAll("[data-value]")) {
        option.setAttribute("aria-checked", String(option.dataset.value === String(value)));
      }
    }
  }
  applyLook();
  renderProviders();
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

for (const el of document.querySelectorAll("[data-setting]")) {
  const name = el.dataset.setting;
  if (el.type === "checkbox") {
    el.addEventListener("change", () => saveSettings({ [name]: el.checked }));
  } else if (el.tagName === "SELECT") {
    el.addEventListener("change", () => {
      const value = el.hasAttribute("data-number") ? Number(el.value) : el.value;
      if (name === "aiProvider") run("set_ai_provider", { provider: value }).then(loadSettings);
      else saveSettings({ [name]: value });
    });
  } else {
    el.addEventListener("click", (event) => {
      const option = event.target.closest("[data-value]");
      if (option) saveSettings({ [name]: option.dataset.value });
    });
  }
}

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

// RAM now.
async function showMemory() {
  const info = await run("get_memory");
  const note = document.getElementById("memory-now");
  if (!info) return;
  const total = info.totalMb == null ? "Ölçülemedi" : info.totalMb >= 1024 ? `${(info.totalMb / 1024).toFixed(1)} GB` : `${info.totalMb} MB`;
  note.textContent = `${total} · ${info.awake} açık, ${info.asleep} uyuyan sekme`;
}
document.getElementById("memory-refresh").addEventListener("click", showMemory);

// Default browser.
const defaultButton = document.getElementById("default-browser");
async function showDefault() {
  const yes = await run("is_default_browser");
  defaultButton.hidden = Boolean(yes);
  document.getElementById("default-status").textContent = yes
    ? "Raven varsayılan tarayıcın. Linkler Raven'da açılıyor"
    : "Başka programlarda tıkladığın linkler Raven'da açılır";
}
defaultButton.addEventListener("click", async () => {
  try {
    await invoke("make_default_browser");
    document.getElementById("default-status").textContent = "Açılan Windows penceresinde Raven'ı seç";
  } catch (error) {
    document.getElementById("default-status").textContent = String(error);
  }
});

// Import from other browsers.
const importButtons = document.getElementById("import-buttons");
const importStatus = document.getElementById("import-status");
async function loadImportSources() {
  const sources = (await run("get_import_sources")) || [];
  importButtons.replaceChildren(
    ...sources.map((source) => {
      const button = document.createElement("button");
      button.className = "secondary";
      button.textContent = source.name;
      button.addEventListener("click", async () => {
        button.disabled = true;
        importStatus.textContent = `${source.name} içe aktarılıyor…`;
        try {
          const result = await invoke("import_browser", { id: source.id });
          importStatus.textContent = `${source.name}: ${result.bookmarks} yer imi, ${result.history} geçmiş kaydı eklendi`;
        } catch (error) {
          importStatus.textContent = String(error);
        } finally {
          button.disabled = false;
        }
      });
      return button;
    }),
  );
  if (sources.length === 0) importStatus.textContent = "Bu bilgisayarda başka tarayıcı bulunamadı";
}

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

// Extensions

const extensionsList = document.getElementById("extensions-list");
const extensionsEmpty = document.getElementById("extensions-empty");
const extensionStatus = document.getElementById("extension-status");
const extensionLink = document.getElementById("extension-link");
const extensionAdd = document.getElementById("extension-add");
const confirmBox = document.getElementById("extension-confirm");
let pendingInstall = null;

const PUZZLE_ICON =
  '<svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M9 4h4v2.5a1.5 1.5 0 0 0 3 0V4h3a1 1 0 0 1 1 1v4h-2.5a1.5 1.5 0 0 0 0 3H20v7a1 1 0 0 1-1 1h-4v-2.5a1.5 1.5 0 0 0-3 0V20H5a1 1 0 0 1-1-1v-6h2.5a1.5 1.5 0 0 0 0-3H4V5a1 1 0 0 1 1-1z"/></svg>';

function setExtensionStatus(text, error = false) {
  extensionStatus.textContent = text;
  extensionStatus.classList.toggle("error", error);
}

async function loadExtensions() {
  let items = [];
  try {
    items = await invoke("get_extensions");
  } catch (error) {
    setExtensionStatus(String(error), true);
  }
  extensionsEmpty.hidden = items.length > 0;
  extensionsList.replaceChildren(
    ...items.map((item) => {
      const row = document.createElement("div");
      row.className = "extension-row";
      row.classList.toggle("off", !item.enabled);

      const icon = document.createElement("span");
      icon.className = "extension-icon";
      if (item.icon) {
        const img = document.createElement("img");
        img.src = item.icon;
        img.alt = "";
        icon.append(img);
      } else {
        icon.innerHTML = PUZZLE_ICON;
      }

      const text = document.createElement("div");
      text.className = "extension-text";
      const name = document.createElement("div");
      name.className = "extension-name";
      name.textContent = item.name || item.id;
      const meta = document.createElement("div");
      meta.className = "extension-meta";
      meta.textContent = item.adblock ? "Reklam engelleyici" : item.enabled ? "Açık" : "Kapalı";
      text.append(name, meta);

      const actions = document.createElement("div");
      actions.className = "download-actions";
      if (item.hasOptions) {
        actions.append(smallButton("Ayarlar", () => {
          closePanel();
          run("open_extension_options", { id: item.id });
        }));
      }
      actions.append(smallButton("Kaldır", async (event) => {
        const button = event.currentTarget;
        if (button.dataset.confirm !== "yes") {
          button.dataset.confirm = "yes";
          button.textContent = "Emin misin?";
          setTimeout(() => {
            button.dataset.confirm = "";
            button.textContent = "Kaldır";
          }, 3000);
          return;
        }
        try {
          await invoke("remove_extension", { id: item.id });
        } catch (error) {
          setExtensionStatus(String(error), true);
        }
      }));

      const toggle = document.createElement("input");
      toggle.type = "checkbox";
      toggle.className = "switch";
      toggle.setAttribute("role", "switch");
      toggle.setAttribute("aria-label", `${item.name} açık`);
      toggle.checked = item.enabled;
      toggle.addEventListener("change", async () => {
        try {
          await invoke("set_extension_enabled", { id: item.id, enabled: toggle.checked });
        } catch (error) {
          toggle.checked = !toggle.checked;
          setExtensionStatus(String(error), true);
        }
      });

      row.append(icon, text, actions, toggle);
      return row;
    }),
  );
}

async function installExtension(input) {
  extensionAdd.disabled = true;
  setExtensionStatus("İndiriliyor…");
  try {
    await invoke("install_extension", { input });
    extensionLink.value = "";
    setExtensionStatus("Eklendi");
  } catch (error) {
    setExtensionStatus(String(error), true);
  } finally {
    extensionAdd.disabled = false;
  }
}

/// "Raven'a ekle" on a store page: ask before adding.
function askToInstall(request) {
  pendingInstall = request;
  document.getElementById("extension-confirm-text").textContent =
    `“${request.title || "Bu uzantı"}” Raven'a eklensin mi?`;
  confirmBox.hidden = false;
  document.getElementById("extension-confirm-yes").focus();
}

document.getElementById("extension-confirm-yes").addEventListener("click", () => {
  confirmBox.hidden = true;
  if (pendingInstall) installExtension(pendingInstall.storeId);
  pendingInstall = null;
});
document.getElementById("extension-confirm-no").addEventListener("click", () => {
  confirmBox.hidden = true;
  pendingInstall = null;
});

document.getElementById("extension-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (extensionLink.value.trim()) installExtension(extensionLink.value);
});
document.getElementById("extensions-store").addEventListener("click", () => {
  closePanel();
  run("open_extension_store");
});

listen("extension-install", ({ payload }) => {
  if (openPanelName !== "extensions") return;
  if (payload.state === "downloading") setExtensionStatus("İndiriliyor…");
  if (payload.state === "adding") setExtensionStatus("Ekleniyor…");
});
listen("extensions-changed", () => {
  if (openPanelName === "extensions") loadExtensions();
});

// Bookmarks page: search, rename, change address, move, remove.

const bookmarksList = document.getElementById("bookmarks-list");
const bookmarksSearch = document.getElementById("bookmarks-search");
const ICON = {
  up: '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 15l6-6 6 6"/></svg>',
  down: '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  edit: '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20h4L19 9l-4-4L4 16z"/></svg>',
  remove: '<svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
};

function iconButton(html, label, onClick) {
  const button = document.createElement("button");
  button.className = "icon-button small";
  button.innerHTML = html;
  button.title = label;
  button.setAttribute("aria-label", label);
  button.addEventListener("click", onClick);
  return button;
}

async function loadBookmarksPage() {
  const all = (await run("get_bookmarks")) || [];
  const query = bookmarksSearch.value.trim().toLocaleLowerCase("tr");
  const items = all.filter((b) => !query || b.title.toLocaleLowerCase("tr").includes(query) || b.url.toLowerCase().includes(query));
  document.getElementById("bookmarks-empty").hidden = all.length > 0;
  bookmarksList.replaceChildren(
    ...items.map((bookmark) => {
      const row = document.createElement("div");
      row.className = "bookmark-row";
      const link = document.createElement("button");
      link.className = "history-link";
      const mark = document.createElement("span");
      mark.className = "mark-small";
      RavenIcons.fill(mark, bookmark.url, bookmark.title);
      const title = document.createElement("span");
      title.className = "history-title";
      title.textContent = bookmark.title;
      const host = document.createElement("span");
      host.className = "history-host";
      host.textContent = RavenIcons.hostOf(bookmark.url);
      link.append(mark, title, host);
      link.addEventListener("click", () => {
        closePanel();
        run("open_url", { url: bookmark.url, newTab: false });
      });
      const index = all.findIndex((b) => b.id === bookmark.id);
      const actions = document.createElement("div");
      actions.className = "row-actions";
      actions.append(
        iconButton(ICON.up, "Yukarı taşı", () => run("move_bookmark", { id: bookmark.id, index: Math.max(0, index - 1) }).then(loadBookmarksPage)),
        iconButton(ICON.down, "Aşağı taşı", () => run("move_bookmark", { id: bookmark.id, index: index + 1 }).then(loadBookmarksPage)),
        iconButton(ICON.edit, "Düzenle", () => {
          const form = document.createElement("form");
          form.className = "bookmark-edit";
          const name = document.createElement("input");
          name.value = bookmark.title;
          name.setAttribute("aria-label", "Ad");
          const url = document.createElement("input");
          url.value = bookmark.url;
          url.setAttribute("aria-label", "Adres");
          const save = document.createElement("button");
          save.className = "primary";
          save.textContent = "Kaydet";
          form.append(name, url, save);
          form.addEventListener("submit", async (event) => {
            event.preventDefault();
            await run("edit_bookmark", { id: bookmark.id, title: name.value, url: url.value });
            loadBookmarksPage();
          });
          row.replaceChildren(form);
          name.focus();
          name.select();
        }),
        iconButton(ICON.remove, "Kaldır", () => run("remove_bookmark", { id: bookmark.id }).then(loadBookmarksPage)),
      );
      row.append(link, actions);
      return row;
    }),
  );
}
bookmarksSearch.addEventListener("input", loadBookmarksPage);
listenHere("open-bookmarks", () => openPanel("bookmarks"));

// Archive: tabs the automatic cleanup closed.

const archiveList = document.getElementById("archive-list");
async function loadArchive() {
  const items = (await run("get_archive")) || [];
  document.getElementById("archive-empty").hidden = items.length > 0;
  archiveList.replaceChildren(
    ...items.map((item, index) => {
      const row = document.createElement("div");
      row.className = "history-row";
      const time = document.createElement("span");
      time.className = "history-time";
      time.textContent = new Date(item.time * 1000).toLocaleDateString("tr-TR", { day: "numeric", month: "short" });
      const link = document.createElement("button");
      link.className = "history-link";
      const mark = document.createElement("span");
      mark.className = "mark-small";
      RavenIcons.fill(mark, item.url, item.title);
      const title = document.createElement("span");
      title.className = "history-title";
      title.textContent = item.title || item.url;
      const host = document.createElement("span");
      host.className = "history-host";
      host.textContent = RavenIcons.hostOf(item.url);
      link.append(mark, title, host);
      link.addEventListener("click", () => {
        closePanel();
        run("open_archived", { index });
      });
      row.append(time, link);
      return row;
    }),
  );
}
document.getElementById("archive-clear").addEventListener("click", async () => {
  await run("clear_archive");
  loadArchive();
});
listen("archive", () => {
  if (openPanelName === "archive") loadArchive();
});

// The AI panel at the right: Claude, ChatGPT or Gemini with your own account.

const aiButton = document.getElementById("ai-button");
const aiHeader = document.getElementById("ai-header");
const aiProviders = aiHeader.querySelector(".ai-providers");
const AI_NAMES = { claude: "Claude", chatgpt: "ChatGPT", gemini: "Gemini" };

function renderProviders() {
  if (!settings) return;
  aiProviders.replaceChildren(
    ...Object.entries(AI_NAMES).map(([id, name]) => {
      const button = document.createElement("button");
      button.className = "ai-provider";
      button.setAttribute("role", "radio");
      button.setAttribute("aria-checked", String(settings.aiProvider === id));
      button.textContent = name;
      button.addEventListener("click", () => run("set_ai_provider", { provider: id }).then(loadSettings));
      return button;
    }),
  );
}

function showAiPanel(open) {
  document.body.classList.toggle("ai-open", open);
  aiHeader.hidden = !open;
  aiButton.setAttribute("aria-pressed", String(open));
}

aiButton.addEventListener("click", () => run("toggle_ai_panel", {}));
document.getElementById("ai-close").addEventListener("click", () => run("toggle_ai_panel", { open: false }));
document.getElementById("ai-reload").addEventListener("click", () => run("reload_ai"));
aiHeader.querySelectorAll("[data-ai]").forEach((chip) =>
  chip.addEventListener("click", async () => {
    try {
      await invoke("ask_ai", { action: chip.dataset.ai, question: null });
    } catch (error) {
      chip.title = String(error);
    }
  }),
);
listenHere("ai-panel", (payload) => showAiPanel(payload.open));

// Full screen (F11, or a video's full screen button).
let hintTimer = null;
listenHere("fullscreen", (payload) => {
  document.body.classList.toggle("fullscreen", payload.on);
  const hint = document.getElementById("fullscreen-hint");
  hint.hidden = !payload.on;
  clearTimeout(hintTimer);
  if (payload.on) hintTimer = setTimeout(() => (hint.hidden = true), 2500);
});

// Hover card: title, address and memory of a tab, like Chrome.
let cardTimer = null;
let cardFor = null;
tabsEl.addEventListener("mouseover", (event) => {
  const el = event.target.closest(".tab");
  if (!el || drag || popupKind === "tab-menu") return;
  const id = Number(el.dataset.id);
  if (cardFor === id) return;
  clearTimeout(cardTimer);
  cardTimer = setTimeout(() => {
    const tab = state.tabs.find((item) => item.id === id);
    if (!tab || (popupKind && popupKind !== "tab-card")) return;
    cardFor = id;
    const rect = el.getBoundingClientRect();
    const vertical = document.body.classList.contains("vertical-tabs");
    openPopup(
      "tab-card",
      { title: tab.title, url: tab.url, memory: tab.memory, asleep: tab.asleep && tab.id !== state.active, audible: tab.audible },
      vertical
        ? { x: rect.right + 10, y: rect.top - 4, width: 300, height: 92, focus: false }
        : { x: Math.max(8, rect.left), y: rect.bottom + 6, width: 300, height: 92, focus: false },
    );
  }, cardFor ? 80 : 550);
});
tabsEl.addEventListener("mouseleave", () => {
  clearTimeout(cardTimer);
  if (popupKind === "tab-card") closePopup();
  cardFor = null;
});
tabsEl.addEventListener("mousedown", () => {
  clearTimeout(cardTimer);
  if (popupKind === "tab-card") closePopup();
  cardFor = null;
});

// Find in page (Ctrl+F): a small bar at the top right of the page.
function pageRect() {
  const slot = document.querySelector(".page-slot").getBoundingClientRect();
  return slot;
}
listenHere("open-find", () => {
  const page = pageRect();
  const width = 380;
  openPopup("find", {}, { x: page.right - width - 12, y: page.top + 10, width, height: 56 });
});

// Command palette (Ctrl+K) and tab search (Ctrl+Shift+A).
function openPalette(mode) {
  const width = Math.min(640, window.innerWidth - 80);
  openPopup("palette", { mode, active: state.active }, { x: (window.innerWidth - width) / 2, y: 70, width, height: 440 });
}
listenHere("open-palette", () => openPalette("all"));
listenHere("open-tab-search", () => openPalette("tabs"));

// Short messages at the bottom of the page ("Ekran görüntüsü kaydedildi"),
// drawn by the popup so they show over the page.
let toastTimer = null;
function showToast(text) {
  if (popupKind && popupKind !== "toast") return;
  const page = pageRect();
  const width = Math.min(520, page.width - 40);
  openPopup("toast", { text }, { x: page.left + (page.width - width) / 2, y: page.bottom - 64, width, height: 46, focus: false });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    if (popupKind === "toast") closePopup();
  }, 3500);
}
listenHere("toast", (payload) => showToast(payload.text));

// Game mode badge.
listen("vertical-tabs", ({ payload }) => document.body.classList.toggle("vertical-tabs", payload));
listen("game-mode", ({ payload }) => document.body.classList.toggle("game-mode", payload));

// Keyboard shortcuts, the same as Chrome and Edge. Inside websites, Raven
// catches them itself (src-tauri/src/shortcuts.rs); these cover the toolbar.
function actionFor(event) {
  if (event.key === "F11") return "fullscreen";
  if (event.key === "F12") return "dev-tools";
  if (!event.ctrlKey || event.altKey) return null;
  const key = event.key.toLowerCase();
  const shift = event.shiftKey;
  if (key === " " && !shift) return "toggle-ai";
  if (key === "f" && !shift) return "find";
  if (key === "p" && !shift) return "print";
  if (key === "s") return shift ? "screenshot" : "save-page";
  if (key === "u" && !shift) return "view-source";
  if (key === "k" && !shift) return "command-palette";
  if (key === "a" && shift) return "tab-search";
  if (key === "i" && shift) return "dev-tools";
  if (key === "t") return shift ? "reopen-tab" : "new-tab";
  if (key === "n") return shift ? "new-private-window" : "new-window";
  if (key === "+" || key === "=") return "zoom-in";
  if (key === "-" || key === "_") return "zoom-out";
  if (key === "0" && !shift) return "zoom-reset";
  if (shift) {
    if (key === "tab") return "previous-tab";
    if (key === "b") return "toggle-bookmarks-bar";
    if (key === "o") return "open-bookmarks";
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

listenHere("focus-address", () => {
  address.focus();
  address.select();
});
