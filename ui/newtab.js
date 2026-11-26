// Raven's new tab page: clock, date, search and shortcuts. It has no access to
// Raven's commands. Shortcuts are kept in this page's own storage; the most
// visited sites come from the toolbar, which copies them to "raven-top-sites".

const clock = document.getElementById("clock");
const date = document.getElementById("date");
const timeFormat = new Intl.DateTimeFormat("tr-TR", { hour: "2-digit", minute: "2-digit" });
const dateFormat = new Intl.DateTimeFormat("tr-TR", { weekday: "long", day: "numeric", month: "long" });

// Same colours as Raven (the toolbar copies the setting here).
try {
  document.documentElement.dataset.appearance = localStorage.getItem("raven-appearance") || "dark";
  document.documentElement.dataset.accent = localStorage.getItem("raven-accent") || "violet";
} catch {}
RavenThemes.applySaved();

const greeting = document.getElementById("greeting");
function greet(hour) {
  if (hour < 5) return "İyi geceler";
  if (hour < 12) return "Günaydın";
  if (hour < 18) return "İyi günler";
  if (hour < 22) return "İyi akşamlar";
  return "İyi geceler";
}

function tick() {
  const now = new Date();
  clock.textContent = timeFormat.format(now);
  date.textContent = dateFormat.format(now);
  greeting.textContent = greet(now.getHours());
}
tick();
setInterval(tick, 1000);

// Same rules as the address bar (src-tauri/src/tabs.rs, to_url). The search
// engine chosen in the settings is copied here by the toolbar.
function searchUrl() {
  try {
    return JSON.parse(localStorage.getItem("raven-search-url")) || "https://www.google.com/search?q=";
  } catch {
    return "https://www.google.com/search?q=";
  }
}

function toUrl(input) {
  const text = input.trim();
  if (!text) return null;
  if (/^https?:\/\//i.test(text)) return text;
  if (text.includes(".") && !text.includes(" ")) return `https://${text}`;
  return `${searchUrl()}${encodeURIComponent(text).replace(/%20/g, "+")}`;
}

// "Sor": the question goes to the AI panel (Raven catches this address).
document.getElementById("ask").addEventListener("click", () => {
  const text = document.getElementById("query").value.trim();
  location.href = "raven-action:ask-ai:" + encodeURIComponent(text);
});

document.getElementById("search").addEventListener("submit", (event) => {
  event.preventDefault();
  const url = toUrl(document.getElementById("query").value);
  if (url) location.href = url;
});

// Shortcuts

const MAX_SHORTCUTS = 10;
const list = document.getElementById("shortcuts");
const template = document.getElementById("shortcut-template");
const menu = document.getElementById("menu");
const editor = document.getElementById("editor");
const editorTitle = document.getElementById("editor-title");
const editorName = document.getElementById("editor-name");
const editorUrl = document.getElementById("editor-url");

function read(key, fallback) {
  try {
    const value = JSON.parse(localStorage.getItem(key));
    return value ?? fallback;
  } catch {
    return fallback;
  }
}

function write(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch (error) {
    console.error(error);
  }
}

/// Your own shortcuts, or null while Raven picks the most visited sites for you.
function ownShortcuts() {
  return read("raven-shortcuts", null);
}

function shortcuts() {
  const own = ownShortcuts();
  if (own) return own;
  const hidden = new Set(read("raven-hidden-sites", []));
  return read("raven-top-sites", [])
    .filter((site) => !hidden.has(site.url))
    .slice(0, 8)
    .map((site) => ({ name: site.title, url: site.url }));
}

/// Any change makes the list yours, like "My shortcuts" in Chrome.
function saveOwn(items) {
  write("raven-shortcuts", items.slice(0, MAX_SHORTCUTS));
  render();
}

let menuIndex = -1;
let editIndex = -1;

function render() {
  const items = shortcuts();
  const tiles = items.map((item, index) => {
    const tile = template.content.firstElementChild.cloneNode(true);
    const link = tile.querySelector(".shortcut-link");
    link.href = item.url;
    link.title = item.url;
    RavenIcons.fill(tile.querySelector(".mark"), item.url, item.name);
    tile.querySelector(".shortcut-name").textContent = item.name || RavenIcons.hostOf(item.url);
    tile.querySelector(".more").addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openMenu(index, event.currentTarget);
    });
    return tile;
  });

  if (items.length < MAX_SHORTCUTS) {
    const add = document.createElement("button");
    add.className = "shortcut add";
    add.innerHTML =
      '<span class="mark add-mark" aria-hidden="true"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></span><span class="shortcut-name">Kısayol ekle</span>';
    add.addEventListener("click", () => openEditor(-1));
    tiles.push(add);
  }
  list.replaceChildren(...tiles);
}

function openMenu(index, button) {
  menuIndex = index;
  const box = button.getBoundingClientRect();
  menu.style.left = `${Math.round(box.left)}px`;
  menu.style.top = `${Math.round(box.bottom + 4)}px`;
  menu.hidden = false;
  document.getElementById("menu-edit").focus();
}

function closeMenu() {
  menu.hidden = true;
}

document.addEventListener("click", (event) => {
  if (!menu.contains(event.target)) closeMenu();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") closeMenu();
});

document.getElementById("menu-edit").addEventListener("click", () => {
  closeMenu();
  openEditor(menuIndex);
});

document.getElementById("menu-remove").addEventListener("click", () => {
  closeMenu();
  const items = shortcuts();
  const removed = items[menuIndex];
  if (!removed) return;
  if (ownShortcuts()) {
    saveOwn(items.filter((_, index) => index !== menuIndex));
  } else {
    // Still the most visited list: just stop showing this site.
    write("raven-hidden-sites", [...read("raven-hidden-sites", []), removed.url]);
    render();
  }
});

function openEditor(index) {
  editIndex = index;
  const item = shortcuts()[index];
  editorTitle.textContent = item ? "Kısayolu düzenle" : "Kısayol ekle";
  editorName.value = item ? item.name : "";
  editorUrl.value = item ? item.url : "";
  editor.showModal();
  (item ? editorName : editorUrl).focus();
}

document.getElementById("editor-cancel").addEventListener("click", () => editor.close());

document.getElementById("editor-form").addEventListener("submit", (event) => {
  const url = toUrl(editorUrl.value);
  if (!url || url.startsWith(searchUrl())) {
    // Not an address.
    event.preventDefault();
    editorUrl.setCustomValidity("Geçerli bir adres yaz, örneğin f2pclub.app");
    editorUrl.reportValidity();
    return;
  }
  const name = editorName.value.trim() || RavenIcons.hostOf(url).replace(/^www\./, "");
  const items = shortcuts();
  if (editIndex >= 0 && items[editIndex]) {
    items[editIndex] = { name, url };
  } else {
    items.push({ name, url });
  }
  saveOwn(items);
});
editorUrl.addEventListener("input", () => editorUrl.setCustomValidity(""));

render();
// The toolbar refreshes the most visited sites now and then.
addEventListener("storage", (event) => {
  if (event.key === "raven-top-sites" && !ownShortcuts()) render();
});


// Customize: background (sky, plain or your picture), clock, greeting, shortcuts.
const custom = document.getElementById("custom");
const look = { bg: "sky", image: null, clock: true, greeting: true, shortcuts: true, ...read("raven-newtab", {}) };

function applyCustom() {
  document.body.dataset.bg = look.bg === "image" && !look.image ? "sky" : look.bg;
  document.body.style.backgroundImage = look.bg === "image" && look.image ? `url("${look.image}")` : "";
  document.getElementById("clock").hidden = !look.clock;
  document.getElementById("date").hidden = !look.clock;
  greeting.hidden = !look.greeting;
  list.hidden = !look.shortcuts;
}
function saveCustom() {
  try {
    localStorage.setItem("raven-newtab", JSON.stringify(look));
  } catch {
    // A picture too big for the page's storage: keep it for this page only.
  }
  applyCustom();
}
applyCustom();

document.getElementById("customize").addEventListener("click", () => {
  for (const radio of custom.querySelectorAll('input[name="bg"]')) radio.checked = radio.value === look.bg;
  document.getElementById("custom-clock").checked = look.clock;
  document.getElementById("custom-greeting").checked = look.greeting;
  document.getElementById("custom-shortcuts").checked = look.shortcuts;
  custom.showModal();
});
custom.addEventListener("change", (event) => {
  const target = event.target;
  if (target.name === "bg") look.bg = target.value;
  if (target.id === "custom-clock") look.clock = target.checked;
  if (target.id === "custom-greeting") look.greeting = target.checked;
  if (target.id === "custom-shortcuts") look.shortcuts = target.checked;
  if (target.id === "custom-image" && target.files[0]) {
    const file = target.files[0];
    if (file.size > 4 * 1024 * 1024) {
      alert("Resim 4 MB'tan küçük olmalı");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      look.image = reader.result;
      look.bg = "image";
      for (const radio of custom.querySelectorAll('input[name="bg"]')) radio.checked = radio.value === "image";
      saveCustom();
    };
    reader.readAsDataURL(file);
    return;
  }
  saveCustom();
});
