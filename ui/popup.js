// Raven's popup: one small webview per window, drawn over the page. The toolbar
// (main.js) opens it with open_popup_command and says what to show ("kind"):
//   menu         the ⋮ menu
//   zoom         the box from the zoom % in the address bar
//   tab-menu     right click on a tab
//   site-info    the lock in the address bar: security and site permissions
//   suggestions  address bar suggestions (the cursor stays in the address bar)
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const WINDOW = window.__TAURI__.window.getCurrentWindow().label;

const root = document.getElementById("root");
root.tabIndex = -1;
let current = null; // { kind, data }

function run(command, args) {
  return invoke(command, args).catch((error) => console.error(command, error));
}

function close() {
  run("close_popup_command");
}

const svg = (body, size = 16) =>
  `<svg aria-hidden="true" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

const ICONS = {
  plus: svg('<path d="M12 5v14M5 12h14"/>'),
  minus: svg('<path d="M5 12h14"/>', 14),
  plusSmall: svg('<path d="M12 5v14M5 12h14"/>', 14),
  window: svg('<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M3 9h18"/>'),
  private: svg('<path d="M3 10h18"/><path d="M5 10l2-6h10l2 6"/><circle cx="8" cy="16" r="3"/><circle cx="16" cy="16" r="3"/><path d="M11 16h2"/>'),
  search: svg('<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>'),
  history: svg('<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l3 2"/>'),
  download: svg('<path d="M12 4v11"/><path d="M7 10l5 5 5-5"/><path d="M5 20h14"/>'),
  star: svg('<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z"/>'),
  puzzle: svg('<path d="M9 4h4v2.5a1.5 1.5 0 0 0 3 0V4h3a1 1 0 0 1 1 1v4h-2.5a1.5 1.5 0 0 0 0 3H20v7a1 1 0 0 1-1 1h-4v-2.5a1.5 1.5 0 0 0-3 0V20H5a1 1 0 0 1-1-1v-6h2.5a1.5 1.5 0 0 0 0-3H4V5a1 1 0 0 1 1-1z"/>'),
  settings: svg('<path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/>'),
  reload: svg('<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>'),
  copy: svg('<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>'),
  pin: svg('<path d="M9 4h6l-1 6 4 4H6l4-4z"/><path d="M12 14v6"/>'),
  sound: svg('<path d="M4 9v6h4l5 4V5L8 9z"/><path d="M16.5 8.5a5 5 0 0 1 0 7"/>'),
  muted: svg('<path d="M4 9v6h4l5 4V5L8 9z"/><path d="M17 9l4 6M21 9l-4 6"/>'),
  close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
  reopen: svg('<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/>'),
  lock: svg('<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>'),
  warning: svg('<path d="M12 4l9 16H3z"/><path d="M12 10v4M12 17v.5"/>'),
  info: svg('<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8v.5"/>'),
  camera: svg('<rect x="3" y="7" width="13" height="10" rx="2"/><path d="M16 11l5-3v8l-5-3"/>'),
  microphone: svg('<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>'),
  location: svg('<path d="M12 21s-7-6.5-7-12a7 7 0 0 1 14 0c0 5.5-7 12-7 12z"/><circle cx="12" cy="9" r="2.5"/>'),
  notifications: svg('<path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4z"/><path d="M10 20a2 2 0 0 0 4 0"/>'),
};

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") node.className = value;
    else if (key === "html") node.innerHTML = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value);
  }
  node.append(...children);
  return node;
}

/// A menu row: icon, label, optional shortcut. `onPick` runs, then the popup closes.
function item(icon, label, shortcut, onPick, { keepOpen = false, disabled = false } = {}) {
  const children = [el("span", { class: "label", text: label })];
  if (shortcut) children.push(el("kbd", { text: shortcut }));
  const button = el("button", { class: "item", role: "menuitem", html: ICONS[icon] || "" }, children);
  button.disabled = disabled;
  button.addEventListener("click", async () => {
    await onPick();
    if (!keepOpen) close();
  });
  return button;
}

const action = (name) => () => run("run_action", { action: name });

function zoomRow(zoom, enabled) {
  const row = el("div", { class: "item zoom-row", role: "group", "aria-label": "Yakınlaştırma", html: ICONS.search }, [
    el("span", { class: "label", text: "Yakınlaştır" }),
    el("button", { class: "step", "aria-label": "Uzaklaştır (Ctrl + eksi)", title: "Uzaklaştır (Ctrl + -)", html: ICONS.minus, onclick: action("zoom-out") }),
    el("button", { class: "zoom-value", title: "%100'e döndür (Ctrl + 0)", text: `${Math.round(zoom * 100)}%`, onclick: action("zoom-reset") }),
    el("button", { class: "step", "aria-label": "Yakınlaştır (Ctrl + artı)", title: "Yakınlaştır (Ctrl + +)", html: ICONS.plusSmall, onclick: action("zoom-in") }),
  ]);
  row.classList.toggle("disabled", !enabled);
  return row;
}

// The ⋮ menu.
function drawMenu(data) {
  root.className = "popup";
  root.replaceChildren(
    item("plus", "Yeni sekme", "Ctrl+T", action("new-tab")),
    item("window", "Yeni pencere", "Ctrl+N", action("new-window")),
    item("private", "Yeni gizli pencere", "Ctrl+Shift+N", action("new-private-window")),
    el("hr"),
    zoomRow(data.zoom ?? 1, data.page),
    el("hr"),
    item("history", "Geçmiş", "Ctrl+H", action("open-history")),
    item("download", "İndirmeler", "Ctrl+J", action("open-downloads")),
    item("puzzle", "Uzantılar", null, action("open-extensions")),
    item("star", data.bar ? "Yer imleri çubuğunu gizle" : "Yer imleri çubuğunu göster", "Ctrl+Shift+B", action("toggle-bookmarks-bar")),
    el("hr"),
    item("settings", "Ayarlar", null, action("open-settings")),
  );
}

// The zoom box: − 110% + Sıfırla, like Chrome's.
function drawZoom(data) {
  root.className = "popup";
  root.replaceChildren(
    el("div", { class: "zoom-box" }, [
      el("button", { class: "step", "aria-label": "Uzaklaştır", title: "Uzaklaştır (Ctrl + -)", html: ICONS.minus, onclick: action("zoom-out") }),
      el("div", { class: "zoom-value", "aria-live": "polite", text: `${Math.round((data.zoom ?? 1) * 100)}%` }),
      el("button", { class: "step", "aria-label": "Yakınlaştır", title: "Yakınlaştır (Ctrl + +)", html: ICONS.plusSmall, onclick: action("zoom-in") }),
      el("button", { class: "reset", text: "Sıfırla", title: "%100'e döndür (Ctrl + 0)", onclick: action("zoom-reset") }),
    ]),
  );
}

// Right click on a tab.
function drawTabMenu(data) {
  const tab = (name) => () => run("tab_action", { id: data.id, action: name });
  root.className = "popup compact";
  root.replaceChildren(
    item("plus", "Sağa yeni sekme", null, tab("new-tab-right")),
    el("hr"),
    item("reload", "Yeniden yükle", "Ctrl+R", tab("reload")),
    item("copy", "Kopyasını aç", null, tab("duplicate")),
    item("pin", data.pinned ? "Sabitlemeyi kaldır" : "Sabitle", null, tab(data.pinned ? "unpin" : "pin")),
    item(data.muted ? "sound" : "muted", data.muted ? "Sitenin sesini aç" : "Siteyi sessize al", null, tab(data.muted ? "unmute" : "mute")),
    el("hr"),
    item("close", "Kapat", "Ctrl+W", tab("close")),
    item("close", "Diğer sekmeleri kapat", null, tab("close-others"), { disabled: !data.others }),
    item("close", "Sağdaki sekmeleri kapat", null, tab("close-right"), { disabled: !data.right }),
    el("hr"),
    item("reopen", "Kapatılan sekmeyi tekrar aç", "Ctrl+Shift+T", tab("reopen-closed")),
  );
}

// The lock in the address bar: is the connection secure, and what may this site use.
const PERMISSION_NAMES = {
  camera: "Kamera",
  microphone: "Mikrofon",
  location: "Konum",
  notifications: "Bildirimler",
};

async function drawSiteInfo(data) {
  let url;
  try {
    url = new URL(data.url);
  } catch {
    url = null;
  }
  const secure = url?.protocol === "https:";
  const web = url && (url.protocol === "https:" || url.protocol === "http:");
  const host = url ? url.hostname.replace(/^www\./, "") || data.url : data.url;

  const security = secure
    ? el("div", { class: "security", html: ICONS.lock }, [el("span", { text: "Bağlantı güvenli" })])
    : web
      ? el("div", { class: "security unsafe", html: ICONS.warning }, [el("span", { text: "Bağlantı güvenli değil" })])
      : el("div", { class: "security", html: ICONS.info }, [el("span", { text: "Bu bir web sitesi değil" })]);
  const note = secure
    ? "Bu siteye gönderdiğin bilgiler (şifreler, kart numaraları) şifrelenir"
    : web
      ? "Bu sitede şifre ya da kart bilgisi girme. Başkaları görebilir"
      : "";

  const head = el("div", { class: "site-head" }, [el("div", { class: "site-host", text: host, title: data.url }), security]);
  if (note) head.append(el("p", { class: "security-note", text: note }));

  const children = [head];
  if (web) {
    const permissions = (await run("get_site_permissions", { url: data.url })) || [];
    if (current?.data !== data) return; // Something else was opened meanwhile.
    children.push(el("hr"), el("div", { class: "section-title", text: "Site izinleri" }));
    for (const { kind, choice } of permissions) {
      const select = el("select", { "aria-label": PERMISSION_NAMES[kind] || kind }, [
        el("option", { value: "ask", text: "Sor" }),
        el("option", { value: "allow", text: "İzin ver" }),
        el("option", { value: "block", text: "Engelle" }),
      ]);
      select.value = choice;
      select.addEventListener("change", () =>
        run("set_site_permission", { url: data.url, kind, choice: select.value }),
      );
      children.push(
        el("label", { class: "permission", html: ICONS[kind] || "" }, [
          el("span", { class: "label", text: PERMISSION_NAMES[kind] || kind }),
          select,
        ]),
      );
    }
    children.push(
      el("div", { class: "site-foot" }, [
        el("button", {
          class: "reset",
          text: "İzinleri sıfırla",
          onclick: async () => {
            await run("reset_site_permissions", { url: data.url });
            drawSiteInfo(data);
          },
        }),
      ]),
    );
  }
  root.className = "popup";
  root.replaceChildren(...children);
}

// Address bar suggestions. The toolbar keeps the keyboard; it tells which row
// is highlighted with popup_update. A click opens the row here.
function shortUrl(url) {
  return url.replace(/^[a-z]+:\/\//, "").replace(/^www\./, "").replace(/\/$/, "");
}

function drawSuggestions(data) {
  root.className = "popup suggestions";
  root.replaceChildren(
    ...data.rows.map((row, index) => {
      const mark = el("span", { class: "mark", "aria-hidden": "true" });
      const children = [mark];
      if (row.kind === "search") {
        mark.classList.add("plain");
        mark.innerHTML = ICONS.search;
        children.push(el("span", { class: "title", text: row.text }), el("span", { class: "kind", text: "Ara" }));
      } else {
        RavenIcons.fill(mark, row.url, row.title);
        children.push(el("span", { class: "title", text: row.title }), el("span", { class: "url", text: shortUrl(row.url) }));
        if (row.kind === "bookmark") children.push(el("span", { class: "kind", html: ICONS.star }));
      }
      const button = el("div", { class: "item suggestion", role: "option" }, children);
      button.classList.toggle("selected", index === data.selected);
      // mousedown, before the address bar loses focus and closes this.
      button.addEventListener("mousedown", (event) => {
        event.preventDefault();
        run("navigate", { input: row.kind === "search" ? row.text : row.url });
        close();
      });
      return button;
    }),
  );
}

function highlight(selected) {
  root.querySelectorAll(".suggestion").forEach((row, index) => row.classList.toggle("selected", index === selected));
}

const DRAW = {
  menu: drawMenu,
  zoom: drawZoom,
  "tab-menu": drawTabMenu,
  "site-info": drawSiteInfo,
  suggestions: drawSuggestions,
};

let visible = false;

async function show(request) {
  if (!request || request.window !== WINDOW) return;
  current = request;
  await DRAW[request.kind]?.(request.data);
  if (current !== request) return;
  // A short fade in when it opens (not on every letter typed for suggestions).
  if (!visible) {
    root.classList.remove("enter");
    void root.offsetWidth;
    root.classList.add("enter");
  }
  visible = true;
  run("popup_ready");
  // Take the focus (so a click elsewhere closes it) without highlighting a button.
  if (request.kind !== "suggestions") root.focus();
}

listen("popup-closed", ({ payload }) => {
  if (payload !== WINDOW) return;
  visible = false;
  current = null;
  root.replaceChildren();
});

listen("popup-show", ({ payload }) => show(payload));
listen("popup-update", ({ payload }) => {
  if (payload.window !== WINDOW || current?.kind !== "suggestions") return;
  highlight(payload.data.selected);
});
// This page may finish loading after the toolbar asked for it.
run("get_popup_request").then(show);

// Keep the zoom numbers up to date while the box or the menu is open.
listen("tabs", ({ payload }) => {
  if (payload.window !== WINDOW || !current) return;
  const active = payload.tabs.find((tab) => tab.id === payload.active);
  const page = Boolean(active && active.url !== "");
  const zoom = page ? active.zoom : 1;
  if (current.kind === "zoom" || current.kind === "menu") {
    current.data = { ...current.data, zoom, page };
    root.querySelectorAll(".zoom-value").forEach((value) => (value.textContent = `${Math.round(zoom * 100)}%`));
    root.querySelector(".zoom-row")?.classList.toggle("disabled", !page);
  }
});

listen("bookmarks-bar", ({ payload }) => {
  if (current?.kind === "menu") {
    current.data = { ...current.data, bar: payload };
  }
});

// Clicking anywhere else, or Escape, closes it. Suggestions never get focus;
// the toolbar closes them.
window.addEventListener("blur", () => {
  if (visible && current && current.kind !== "suggestions") close();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") close();
  // Arrow keys move through the buttons (a list box keeps its own arrows).
  if ((event.key === "ArrowDown" || event.key === "ArrowUp") && document.activeElement?.tagName !== "SELECT") {
    event.preventDefault();
    const items = [...root.querySelectorAll("button:not(:disabled), select")];
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement);
    const next = event.key === "ArrowDown" ? index + 1 : index === -1 ? items.length - 1 : index - 1;
    items[(next + items.length) % items.length].focus();
  }
});
