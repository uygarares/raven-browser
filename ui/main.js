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

function siteLetter(tab) {
  try {
    const host = new URL(tab.url).hostname.replace(/^www\./, "");
    return host.charAt(0).toUpperCase();
  } catch {
    return (tab.title || "?").charAt(0).toUpperCase();
  }
}

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
        icon.textContent = siteLetter(tab);
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
}

listen("tabs", ({ payload }) => {
  state = payload;
  render();
});

invoke("get_tabs").then((payload) => {
  state = payload;
  render();
});

// Keyboard shortcuts, the same as Chrome and Edge. Inside websites, Raven
// catches them itself (src-tauri/src/shortcuts.rs); these cover the toolbar.
function actionFor(event) {
  if (!event.ctrlKey || event.altKey) return null;
  const key = event.key.toLowerCase();
  const shift = event.shiftKey;
  if (key === "t") return shift ? "reopen-tab" : "new-tab";
  if (shift) return key === "tab" ? "previous-tab" : null;
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
    address.focus();
  } else {
    run("run_action", { action });
  }
});

listen("focus-address", () => {
  address.focus();
  address.select();
});
