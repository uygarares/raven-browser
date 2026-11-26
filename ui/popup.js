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

// Same colours as the toolbar.
function applyLook() {
  try {
    document.documentElement.dataset.appearance = localStorage.getItem("raven-appearance") || "dark";
    document.documentElement.dataset.accent = localStorage.getItem("raven-accent") || "violet";
  } catch {}
}
applyLook();
listen("settings", () => setTimeout(applyLook, 50));
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
  shield: svg('<path d="M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6z"/><path d="M9 12l2 2 4-4"/>'),
  sparkle: svg('<path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z"/><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z"/>'),
  book: svg('<path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z"/><path d="M4 19V5M9 7h6"/>'),
  print: svg('<path d="M7 9V3h10v6"/><rect x="3" y="9" width="18" height="8" rx="2"/><path d="M7 14h10v7H7z"/>'),
  command: svg('<path d="M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z"/>'),
  archive: svg('<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9M10 13h4"/>'),
  moon: svg('<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>'),
  split: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>'),
  play: svg('<path d="M7 5l12 7-12 7z"/>', 14),
  pause: svg('<path d="M8 5v14M16 5v14"/>', 14),
  music: svg('<path d="M9 18V5l11-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="17" cy="16" r="3"/>'),
  chevronUp: svg('<path d="M6 15l6-6 6 6"/>', 14),
  chevronDown: svg('<path d="M6 9l6 6 6-6"/>', 14),
  tab: svg('<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M3 9h8V5"/>'),
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
    item("sparkle", "Yapay zekaya sor", "Ctrl+Space", action("toggle-ai")),
    item("search", "Sayfada bul", "Ctrl+F", action("find"), { disabled: !data.page }),
    item("book", "Okuma görünümü", null, action("reader-mode"), { disabled: !data.page }),
    item("print", "Yazdır", "Ctrl+P", action("print"), { disabled: !data.page }),
    item("camera", "Ekran görüntüsü al", "Ctrl+Shift+S", action("screenshot"), { disabled: !data.page }),
    item("command", "Komut paleti", "Ctrl+K", action("command-palette")),
    el("hr"),
    item("history", "Geçmiş", "Ctrl+H", action("open-history")),
    item("download", "İndirmeler", "Ctrl+J", action("open-downloads")),
    item("star", "Yer imleri", "Ctrl+Shift+O", action("open-bookmarks")),
    item("puzzle", "Uzantılar", null, action("open-extensions")),
    item("archive", "Arşiv", null, action("open-archive")),
    el("hr"),
    item("moon", "Diğer sekmeleri uyut", null, action("sleep-other-tabs")),
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
    ...(data.split != null
      ? [item("split", "Yan yana görünümü kapat", null, tab("unsplit"))]
      : data.active
        ? []
        : [item("split", "Yan yana aç", null, tab("split"))]),
    el("hr"),
    item("reload", "Yeniden yükle", "Ctrl+R", tab("reload")),
    item("copy", "Kopyasını aç", null, tab("duplicate")),
    item("pin", data.pinned ? "Sabitlemeyi kaldır" : "Sabitle", null, tab(data.pinned ? "unpin" : "pin")),
    item(data.muted ? "sound" : "muted", data.muted ? "Sitenin sesini aç" : "Siteyi sessize al", null, tab(data.muted ? "unmute" : "mute")),
    el("hr"),
    ...(data.pinned
      ? []
      : [
          item("plus", "Yeni gruba ekle", null, tab("group-new")),
          ...(data.groups || []).map((group) =>
            item("tab", `Gruba ekle: ${group.name || "Adsız grup"}`, null, tab(`group-${group.id}`)),
          ),
          ...(data.group != null ? [item("close", "Gruptan çıkar", null, tab("ungroup"))] : []),
          el("hr"),
        ]),
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
    const cookies = el("span", { class: "label", text: "Çerezler ve site verileri" });
    run("get_site_cookies").then((count) => {
      if (typeof count === "number") cookies.textContent = `${count} çerez ve site verileri`;
    });
    const clear = el("button", {
      class: "reset",
      text: "Sil",
      title: "Bu sitenin çerezlerini ve kayıtlı verilerini sil (oturumun kapanır)",
      onclick: async () => {
        clear.disabled = true;
        const removed = await run("clear_site_data");
        cookies.textContent = typeof removed === "number" ? `${removed} çerez silindi` : "Silindi";
      },
    });
    children.push(
      el("div", { class: "permission", html: ICONS.archive }, [cookies, clear]),
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

// Raven Shield: an orb with the number blocked on this page, a switch for
// this site, and totals.
function formatCount(n) {
  return n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)} M` : n >= 10_000 ? `${Math.round(n / 1000)} B` : n.toLocaleString("tr-TR");
}

function drawAdblock(data) {
  const lit = data.on && data.siteOn;
  const box = el("div", { class: "shield-box" + (lit ? "" : " off") });
  const orb = el("div", { class: "shield-orb" }, [
    el("div", {}, [
      el("span", { class: "shield-count", text: lit ? formatCount(data.page || 0) : "—" }),
      el("span", { class: "shield-unit", text: lit ? "engellendi" : "kapalı" }),
    ]),
  ]);
  let state = "Bu sayfadaki reklamlar ve izleyiciler";
  if (!data.on) state = "Kalkan Ayarlar'dan kapatılmış";
  else if (!data.siteOn) state = "Bu sitede kapalı";
  else if (data.error) state = data.error;
  else if (!data.ready) state = "Engelleme listeleri hazırlanıyor…";
  box.append(orb, el("div", { class: "shield-site", text: data.site || "Bu sayfa" }), el("div", { class: "shield-state", text: state }));

  const toggle = el("input", { type: "checkbox", class: "switch", role: "switch", "aria-label": "Bu sitede kalkan" });
  toggle.checked = Boolean(data.siteOn);
  toggle.disabled = !data.site || !data.on;
  toggle.addEventListener("change", async () => {
    await run("set_shield_for_site", { on: toggle.checked });
    const fresh = await run("get_shield");
    if (fresh) drawAdblock(fresh);
  });
  const children = [
    box,
    el("label", { class: "shield-toggle" }, [el("span", { text: "Bu sitede kalkan" }), toggle]),
    el("div", { class: "shield-stats" }, [
      el("div", { class: "shield-stat" }, [el("b", { text: formatCount(data.total || 0) }), el("span", { text: "toplam engellenen" })]),
      el("div", { class: "shield-stat" }, [el("b", { text: `~${formatCount(Math.round(((data.total || 0) * 40) / 1024))} MB` }), el("span", { text: "tasarruf" })]),
    ]),
  ];
  if (data.error) {
    children.push(
      el("div", { class: "box-foot" }, [
        el("button", {
          class: "reset",
          text: "Listeleri tekrar indir",
          onclick: async (event) => {
            event.currentTarget.disabled = true;
            event.currentTarget.textContent = "İndiriliyor…";
            await run("update_shield_lists");
            const fresh = await run("get_shield");
            if (fresh) drawAdblock(fresh);
          },
        }),
      ]),
    );
  }
  root.className = "popup";
  root.replaceChildren(...children);
}

// The puzzle: added extensions, quick on/off.
function drawExtensions(data) {
  const rows = data.items.map((ext) => {
    const icon = el("span", { class: "ext-icon", html: ext.icon ? "" : ICONS.puzzle });
    if (ext.icon) icon.append(el("img", { src: ext.icon, alt: "" }));
    const toggle = el("input", { type: "checkbox", class: "switch", role: "switch", "aria-label": `${ext.name} açık` });
    toggle.checked = ext.enabled;
    toggle.addEventListener("change", async () => {
      try {
        await invoke("set_extension_enabled", { id: ext.id, enabled: toggle.checked });
      } catch {
        toggle.checked = !toggle.checked;
      }
    });
    const name = el("span", { class: "label", text: ext.name || ext.id, title: ext.name });
    const row = el("div", { class: "ext-row" }, [icon, name]);
    if (ext.hasOptions) {
      row.append(el("button", {
        class: "step",
        title: "Ayarlar",
        "aria-label": `${ext.name} ayarları`,
        html: ICONS.settings,
        onclick: async () => {
          await run("open_extension_options", { id: ext.id });
          close();
        },
      }));
    }
    row.append(toggle);
    return row;
  });
  root.className = "popup";
  root.replaceChildren(
    el("div", { class: "box-head", html: ICONS.puzzle }, [el("span", { class: "label", text: "Uzantılar" })]),
    ...(rows.length ? rows : [el("p", { class: "box-text muted", text: "Henüz uzantı yok. Mağazada “Raven'a ekle” ile ekle" })]),
    el("hr"),
    item("settings", "Uzantıları yönet", null, action("open-extensions")),
  );
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

// Find in page: query, "3 / 12", previous, next, close.
function drawFind(data) {
  const input = el("input", { class: "find-input", type: "text", placeholder: "Sayfada bul", "aria-label": "Sayfada bul", spellcheck: "false" });
  const count = el("span", { class: "find-count", "aria-live": "polite" });
  let timer = null;
  const search = async (step) => {
    try {
      const found = await invoke("find_in_page", { query: input.value, step });
      count.textContent = input.value ? (found.total ? `${found.current} / ${found.total}` : "Yok") : "";
      root.classList.toggle("none", Boolean(input.value) && !found.total);
    } catch {
      count.textContent = "";
    }
  };
  input.addEventListener("input", () => {
    clearTimeout(timer);
    timer = setTimeout(() => search(0), 120);
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      search(event.shiftKey ? -1 : 1);
    }
  });
  root.className = "popup find";
  root.replaceChildren(
    el("div", { class: "find-bar" }, [
      input,
      count,
      el("button", { class: "step", "aria-label": "Önceki (Shift + Enter)", title: "Önceki", html: ICONS.chevronUp, onclick: () => search(-1) }),
      el("button", { class: "step", "aria-label": "Sonraki (Enter)", title: "Sonraki", html: ICONS.chevronDown, onclick: () => search(1) }),
      el("button", { class: "step", "aria-label": "Kapat (Esc)", title: "Kapat", html: ICONS.close, onclick: close }),
    ]),
  );
  setTimeout(() => input.focus(), 0);
}

// Command palette (Ctrl+K): tabs, bookmarks, history and Raven's commands.
const COMMANDS = [
  ["plus", "Yeni sekme", "new-tab", "Ctrl+T"],
  ["window", "Yeni pencere", "new-window", "Ctrl+N"],
  ["private", "Yeni gizli pencere", "new-private-window", "Ctrl+Shift+N"],
  ["sparkle", "Yapay zeka panelini aç/kapat", "toggle-ai", "Ctrl+Space"],
  ["sparkle", "Bu sayfayı özetle", "ai:summarize", ""],
  ["sparkle", "Bu sayfayı açıkla", "ai:explain", ""],
  ["sparkle", "Bu sayfayı Türkçeye çevir", "ai:translate", ""],
  ["search", "Sayfada bul", "find", "Ctrl+F"],
  ["book", "Okuma görünümü", "reader-mode", ""],
  ["print", "Yazdır", "print", "Ctrl+P"],
  ["camera", "Ekran görüntüsü al", "screenshot", "Ctrl+Shift+S"],
  ["download", "Sayfayı kaydet", "save-page", "Ctrl+S"],
  ["command", "Sayfa kaynağını göster", "view-source", "Ctrl+U"],
  ["star", "Yer imlerine ekle / çıkar", "toggle-bookmark", "Ctrl+D"],
  ["history", "Geçmiş", "open-history", "Ctrl+H"],
  ["star", "Yer imleri", "open-bookmarks", "Ctrl+Shift+O"],
  ["star", "Yer imleri çubuğunu göster / gizle", "toggle-bookmarks-bar", "Ctrl+Shift+B"],
  ["download", "İndirmeler", "open-downloads", "Ctrl+J"],
  ["puzzle", "Uzantılar", "open-extensions", ""],
  ["archive", "Arşiv", "open-archive", ""],
  ["moon", "Diğer sekmeleri uyut", "sleep-other-tabs", ""],
  ["split", "Yan yana görünümü kapat", "unsplit", ""],
  ["window", "Tam ekran", "fullscreen", "F11"],
  ["settings", "Ayarlar", "open-settings", ""],
  ["command", "Geliştirici araçları", "dev-tools", "F12"],
];

function fold(text) {
  return (text || "").toLocaleLowerCase("tr").normalize("NFD").replace(/[\u0300-\u036f]/g, "").replace(/ı/g, "i");
}

async function drawPalette(data) {
  const tabsOnly = data.mode === "tabs";
  const input = el("input", {
    class: "palette-input",
    type: "text",
    placeholder: tabsOnly ? "Açık sekmelerde ara" : "Sekme, yer imi, geçmiş ya da komut ara",
    "aria-label": "Ara",
    spellcheck: "false",
  });
  const list = el("div", { class: "palette-list", role: "listbox" });
  let rows = [];
  let selected = 0;
  let token = 0;
  const tabState = (await run("get_tabs")) || { tabs: [] };

  const pick = async (row) => {
    if (!row) return;
    close();
    if (row.kind === "tab") run("switch_tab", { id: row.id });
    else if (row.kind === "command" && row.action.startsWith("ai:")) run("ask_ai", { action: row.action.slice(3), question: null });
    else if (row.kind === "command") run("run_action", { action: row.action });
    else run("navigate", { input: row.url });
  };

  const render = () => {
    list.replaceChildren(
      ...rows.map((row, index) => {
        const mark = el("span", { class: "mark" });
        if (row.kind === "command") {
          mark.classList.add("plain");
          mark.innerHTML = ICONS[row.icon] || "";
        } else if (row.url) {
          RavenIcons.fill(mark, row.url, row.title);
        } else {
          mark.classList.add("plain");
          mark.innerHTML = ICONS.tab;
        }
        const children = [mark, el("span", { class: "title", text: row.title })];
        if (row.detail) children.push(el("span", { class: "url", text: row.detail }));
        children.push(el("span", { class: "palette-kind", text: row.label }));
        const node = el("div", { class: "item suggestion", role: "option" }, children);
        node.classList.toggle("selected", index === selected);
        node.addEventListener("mousedown", (event) => {
          event.preventDefault();
          pick(row);
        });
        node.addEventListener("mousemove", () => {
          if (selected !== index) {
            selected = index;
            list.querySelectorAll(".suggestion").forEach((n, i) => n.classList.toggle("selected", i === selected));
          }
        });
        return node;
      }),
    );
    if (!rows.length) list.append(el("p", { class: "box-text muted", text: "Sonuç yok" }));
  };

  const update = async () => {
    const query = fold(input.value.trim());
    const my = ++token;
    const tabs = tabState.tabs
      .filter((tab) => !query || fold(tab.title).includes(query) || fold(tab.url).includes(query))
      .map((tab) => ({ kind: "tab", id: tab.id, url: tab.url, title: tab.title, detail: shortUrl(tab.url), label: tab.id === tabState.active ? "Açık" : "Sekme" }));
    const commands = tabsOnly
      ? []
      : COMMANDS.filter(([, name]) => !query || fold(name).includes(query)).map(([icon, title, act, keys]) => ({
          kind: "command",
          icon,
          title,
          action: act,
          label: keys || "Komut",
        }));
    let pages = [];
    if (!tabsOnly && query) {
      const result = await run("get_suggestions", { query: input.value.trim() });
      if (my !== token) return;
      pages = (result?.items || []).map((item) => ({
        kind: "page",
        url: item.url,
        title: item.title,
        detail: shortUrl(item.url),
        label: item.kind === "bookmark" ? "Yer imi" : "Geçmiş",
      }));
    }
    rows = query ? [...tabs.slice(0, 6), ...commands.slice(0, 6), ...pages] : tabsOnly ? tabs : [...tabs.slice(0, 5), ...commands.slice(0, 8)];
    rows = rows.slice(0, 40);
    selected = 0;
    render();
  };

  input.addEventListener("input", update);
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      if (!rows.length) return;
      selected = (selected + (event.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
      list.querySelectorAll(".suggestion").forEach((n, i) => n.classList.toggle("selected", i === selected));
      list.querySelectorAll(".suggestion")[selected]?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      pick(rows[selected]);
    }
  });

  root.className = "popup palette";
  root.replaceChildren(
    el("div", { class: "palette-head", html: ICONS.search }, [input, el("kbd", { text: "Esc" })]),
    list,
  );
  await update();
  setTimeout(() => input.focus(), 0);
}

// Hover card of a tab: title, address, memory.
function drawTabCard(data) {
  const facts = [];
  if (data.asleep) facts.push("Uyuyor · RAM kullanmıyor");
  else if (data.memory != null) facts.push(`${data.memory} MB bellek`);
  if (data.audible) facts.push("Ses çalıyor");
  root.className = "popup card";
  root.replaceChildren(
    el("div", { class: "card-title", text: data.title || "Yeni sekme" }),
    el("div", { class: "card-url", text: data.url ? shortUrl(data.url) : "Raven" }),
    el("div", { class: "card-facts" + (data.asleep ? " asleep" : ""), text: facts.join(" · ") || " " }),
  );
}

// Right click on a group's chip: name, colour, new tab, ungroup, close.
const GROUP_COLORS = { blue: "Mavi", violet: "Mor", pink: "Pembe", red: "Kırmızı", orange: "Turuncu", yellow: "Sarı", green: "Yeşil", cyan: "Turkuaz", grey: "Gri" };
function drawGroupMenu(data) {
  const group = (action, extra = {}) => run("group_command", { group: data.id, action, ...extra });
  const name = el("input", { class: "group-name", type: "text", placeholder: "Grubun adı", "aria-label": "Grubun adı", spellcheck: "false" });
  name.value = data.name || "";
  let timer = null;
  name.addEventListener("input", () => {
    clearTimeout(timer);
    timer = setTimeout(() => group("rename", { name: name.value }), 250);
  });
  name.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      group("rename", { name: name.value });
      close();
    }
  });
  const colors = el(
    "div",
    { class: "group-colors", role: "radiogroup", "aria-label": "Renk" },
    Object.entries(GROUP_COLORS).map(([id, label]) => {
      const button = el("button", { class: "group-color", "data-color": id, role: "radio", "aria-label": label, title: label });
      button.setAttribute("aria-checked", String(id === data.color));
      button.addEventListener("click", () => {
        group("color", { color: id });
        colors.querySelectorAll("button").forEach((b) => b.setAttribute("aria-checked", String(b === button)));
      });
      return button;
    }),
  );
  root.className = "popup compact";
  root.replaceChildren(
    el("div", { class: "group-head" }, [name]),
    colors,
    el("hr"),
    item("plus", "Bu gruba yeni sekme", null, () => group("new-tab")),
    item("tab", "Grubu çöz", null, () => group("ungroup")),
    item("close", "Grubu kapat", null, () => group("close")),
  );
  setTimeout(() => {
    name.focus();
    name.select();
  }, 0);
}

// Media: tabs playing sound, with pause/play, mute and "go to tab".
function drawMedia(data) {
  const rows = data.items.map((item) => {
    const mark = el("span", { class: "ext-icon" });
    RavenIcons.fill(mark, item.url, item.title);
    let playing = item.playing;
    const play = el("button", { class: "step", title: playing ? "Duraklat" : "Oynat", html: playing ? ICONS.pause : ICONS.play });
    play.addEventListener("click", async () => {
      await run("media_command", { id: item.id, action: playing ? "pause" : "play" });
      playing = !playing;
      play.innerHTML = playing ? ICONS.pause : ICONS.play;
      play.title = playing ? "Duraklat" : "Oynat";
      emitPaused(item.id, !playing);
    });
    const mute = el("button", { class: "step", title: item.muted ? "Sesi aç" : "Sessize al", html: item.muted ? ICONS.muted : ICONS.sound });
    mute.addEventListener("click", () => {
      item.muted = !item.muted;
      run("tab_action", { id: item.id, action: item.muted ? "mute" : "unmute" });
      mute.innerHTML = item.muted ? ICONS.muted : ICONS.sound;
    });
    const text = el("button", { class: "media-text", title: "Sekmeye git" }, [
      el("span", { class: "media-title", text: item.title }),
      el("span", { class: "media-site", text: shortUrl(item.url).split("/")[0] }),
    ]);
    text.addEventListener("click", () => {
      run("switch_tab", { id: item.id });
      close();
    });
    return el("div", { class: "media-row" }, [mark, text, play, mute]);
  });
  root.className = "popup";
  root.replaceChildren(el("div", { class: "box-head", html: ICONS.music }, [el("span", { class: "label", text: "Çalan medya" })]), ...rows);
}
function emitPaused(id, paused) {
  window.__TAURI__.event.emit("media-paused", { window: WINDOW, id, paused });
}

// Right click on a bookmark in the bar.
function drawBookmarkMenu(data) {
  root.className = "popup compact";
  root.replaceChildren(
    item("plus", "Yeni sekmede aç", null, () => run("open_url", { url: data.url, newTab: true })),
    item("split", "Burada aç", null, () => run("open_url", { url: data.url, newTab: false })),
    el("hr"),
    item("settings", "Düzenle", null, action("open-bookmarks")),
    item("close", "Kaldır", null, () => run("remove_bookmark", { id: data.id })),
  );
}

function drawToast(data) {
  root.className = "popup toast";
  root.replaceChildren(el("span", { class: "label", text: data.text }));
}

const DRAW = {
  "bookmark-menu": drawBookmarkMenu,
  media: drawMedia,
  "group-menu": drawGroupMenu,
  toast: drawToast,
  find: drawFind,
  palette: drawPalette,
  "tab-card": drawTabCard,
  menu: drawMenu,
  zoom: drawZoom,
  "tab-menu": drawTabMenu,
  "site-info": drawSiteInfo,
  adblock: drawAdblock,
  extensions: drawExtensions,
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
  if (!["suggestions", "tab-card", "find", "palette", "toast"].includes(request.kind)) root.focus();
}

listen("popup-closed", ({ payload }) => {
  if (payload !== WINDOW) return;
  if (current?.kind === "find") run("stop_finding");
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
  if (visible && current && !["suggestions", "tab-card", "find", "toast"].includes(current.kind)) close();
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
