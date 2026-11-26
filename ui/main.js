// Raven toolbar: sends the user's actions to Raven and shows the open address.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const address = document.getElementById("address");
const loading = document.getElementById("loading");

function run(command, args) {
  invoke(command, args).catch((error) => console.error(command, error));
}

document.getElementById("back").addEventListener("click", () => run("go_back"));
document.getElementById("forward").addEventListener("click", () => run("go_forward"));
document.getElementById("reload").addEventListener("click", () => run("reload"));

document.getElementById("address-form").addEventListener("submit", (event) => {
  event.preventDefault();
  run("navigate", { input: address.value });
  address.blur();
});

// Select the whole address when clicked, like other browsers.
address.addEventListener("focus", () => address.select());

listen("content-page", ({ payload }) => {
  if (document.activeElement !== address) {
    address.value = payload.url;
  }
  loading.classList.toggle("on", payload.state === "loading");
});
