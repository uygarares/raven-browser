// Raven's new tab page: clock, date and search. It has no access to Raven's
// commands; it only shows the time and opens addresses.

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
