// Applies the saved theme before the page draws, so it never flashes the wrong one.
// The toolbar keeps a copy of the theme in localStorage; the new tab page reads it here.
(function () {
  try {
    const saved = JSON.parse(localStorage.getItem("raven-theme") || "{}");
    const root = document.documentElement;
    root.dataset.theme = saved.theme === "glass" ? "glass" : "solid";
    if (typeof saved.glassTint === "number") {
      root.style.setProperty("--tint", String(saved.glassTint));
    }
  } catch (error) {
    document.documentElement.dataset.theme = "solid";
  }
})();
