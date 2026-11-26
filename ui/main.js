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
async function openNewTab() {
  await run("new_tab");
  address.focus();
}

document.getElementById("new-tab").addEventListener("click", openNewTab);

function closeTab(id) {
  run("close_tab", { id });
}

function render() {
  tabsEl.replaceChildren(
    ...state.tabs.map((tab) => {
      const el = template.content.firstElementChild.cloneNode(true);
      const isActive = tab.id === state.active;
      el.classList.toggle("active", isActive);
      el.classList.toggle("loading", tab.loading);
      el.setAttribute("aria-selected", String(isActive));
      el.title = tab.title;
      el.querySelector(".tab-title").textContent = tab.title;
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
    loading.classList.toggle("on", active.loading);
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

// Keyboard shortcuts (while Raven's toolbar has focus)
document.addEventListener("keydown", (event) => {
  if (!event.ctrlKey) return;
  const key = event.key.toLowerCase();
  if (key === "t") {
    event.preventDefault();
    openNewTab();
  } else if (key === "w" && state.active != null) {
    event.preventDefault();
    closeTab(state.active);
  } else if (key === "l") {
    event.preventDefault();
    address.focus();
  }
});
