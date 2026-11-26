// Raven's ⋮ menu. It is its own small webview on top of the page, so the page
// stays visible behind it. It closes when it loses focus.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const zoomRow = document.querySelector(".zoom-row");
const zoomValue = document.getElementById("zoom-value");
const barLabel = document.getElementById("bar-label");

function close() {
  invoke("close_menu");
}

document.querySelectorAll("[data-action]").forEach((button) => {
  button.addEventListener("click", async () => {
    await invoke("run_action", { action: button.dataset.action });
    // Zoom buttons keep the menu open so you can press them again, like Chrome.
    if (!button.hasAttribute("data-keep-open")) close();
  });
});

function showTabs(state) {
  const active = state.tabs.find((tab) => tab.id === state.active);
  const page = active && active.url !== "";
  zoomRow.classList.toggle("disabled", !page);
  zoomValue.textContent = `${Math.round((page ? active.zoom : 1) * 100)}%`;
}

function showBar(shown) {
  barLabel.textContent = shown ? "Yer imleri çubuğunu gizle" : "Yer imleri çubuğunu göster";
}

listen("tabs", ({ payload }) => showTabs(payload));
listen("bookmarks-bar", ({ payload }) => showBar(payload));

function refresh() {
  invoke("get_tabs").then(showTabs);
  invoke("get_bookmarks_bar").then(showBar);
  document.querySelector(".item").focus();
}
refresh();
listen("menu-opened", refresh);

// Clicking anywhere else, or Escape, closes the menu.
window.addEventListener("blur", close);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") close();
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    const items = [...document.querySelectorAll("button")];
    const index = items.indexOf(document.activeElement);
    const next = event.key === "ArrowDown" ? index + 1 : index - 1;
    items[(next + items.length) % items.length].focus();
  }
});
