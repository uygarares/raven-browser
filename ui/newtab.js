// Raven's new tab page: clock, date, search and shortcuts. It has no access to
// Raven's commands. Shortcuts are kept in this page's own storage; the most
// visited sites come from the toolbar, which copies them to "raven-top-sites".

const clock = document.getElementById("clock");
const date = document.getElementById("date");
const timeFormat = new Intl.DateTimeFormat("tr-TR", { hour: "2-digit", minute: "2-digit" });
const dateFormat = new Intl.DateTimeFormat("tr-TR", { weekday: "long", day: "numeric", month: "long" });

function tick() {
  const now = new Date();
  clock.textContent = timeFormat.format(now);
  date.textContent = dateFormat.format(now);
}
tick();
setInterval(tick, 1000);

// Same rules as the address bar (src-tauri/src/tabs.rs, to_url).
function toUrl(input) {
  const text = input.trim();
  if (!text) return null;
  if (/^https?:\/\//i.test(text)) return text;
  if (text.includes(".") && !text.includes(" ")) return `https://${text}`;
  return `https://www.google.com/search?q=${encodeURIComponent(text)}`;
}

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
  if (!url || url.startsWith("https://www.google.com/search")) {
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
