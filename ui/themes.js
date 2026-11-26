// Raven's themes: the ink colours of the frame and the three lights of the
// feather sheen. Built-in ones are here; community themes come from
// themes/index.json in Raven's GitHub repository (the theme store).
const RavenThemes = (() => {
  const BUILT_IN = [
    { id: "raven", name: "Kuzgun", author: "Raven", colors: { ink: "#0d0c1a", ink2: "#14132a", page: "#111020", menu: "#19182f", violet: "#8f6cf2", blue: "#4f7dff", teal: "#2fd0b8" } },
    { id: "midnight", name: "Gece Mavisi", author: "Raven", colors: { ink: "#070d1a", ink2: "#0e1628", page: "#0a1222", menu: "#121d33", violet: "#4f7dff", blue: "#38c6ff", teal: "#7cf0d6" } },
    { id: "rose", name: "Gül", author: "Raven", colors: { ink: "#170b13", ink2: "#22111c", page: "#1b0e17", menu: "#291624", violet: "#ff6fa8", blue: "#b06cf2", teal: "#ffb36b" } },
    { id: "forest", name: "Orman", author: "Raven", colors: { ink: "#08120d", ink2: "#0f1c15", page: "#0b1711", menu: "#13241b", violet: "#4cd07d", blue: "#2fb8a0", teal: "#c8e06c" } },
    { id: "ember", name: "Kor", author: "Raven", colors: { ink: "#150c09", ink2: "#201410", page: "#1a0f0b", menu: "#281912", violet: "#ff7a45", blue: "#ffb347", teal: "#ff4f6d" } },
    { id: "graphite", name: "Grafit", author: "Raven", colors: { ink: "#101113", ink2: "#18191c", page: "#131416", menu: "#1d1f22", violet: "#9aa0ac", blue: "#c3c7cf", teal: "#7d828c" } },
  ];
  const STORE = "https://raw.githubusercontent.com/uygarares/raven-browser/main/themes/index.json";
  const COLOR = /^#[0-9a-f]{6}$/i;
  const KEYS = ["ink", "ink2", "page", "menu", "violet", "blue", "teal"];

  function valid(theme) {
    return theme && typeof theme.id === "string" && typeof theme.name === "string" && theme.colors && KEYS.every((key) => COLOR.test(theme.colors[key] || ""));
  }

  /// Puts a theme's colours on this page. Light appearance keeps Raven's light
  /// surfaces and takes only the sheen.
  function apply(theme, appearance) {
    if (!valid(theme)) theme = BUILT_IN[0];
    const style = document.documentElement.style;
    const c = theme.colors;
    style.setProperty("--sheen-violet", c.violet);
    style.setProperty("--sheen-blue", c.blue);
    style.setProperty("--sheen-teal", c.teal);
    const dark = appearance !== "light";
    for (const [name, key] of [["--ink", "ink"], ["--ink-2", "ink2"], ["--page", "page"], ["--menu", "menu"]]) {
      if (dark) style.setProperty(name, c[key]);
      else style.removeProperty(name);
    }
  }

  /// The theme the toolbar saved, for the menus and the new tab page.
  function applySaved() {
    try {
      const theme = JSON.parse(localStorage.getItem("raven-theme") || "null");
      apply(theme, localStorage.getItem("raven-appearance") || "dark");
    } catch {
      apply(null, "dark");
    }
  }

  function remember(theme) {
    try {
      localStorage.setItem("raven-theme", JSON.stringify(theme));
    } catch {}
  }

  async function community() {
    const response = await fetch(STORE, { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const data = await response.json();
    return (data.themes || []).filter(valid).filter((t) => !BUILT_IN.some((b) => b.id === t.id)).slice(0, 200);
  }

  return { BUILT_IN, apply, applySaved, remember, community, valid };
})();
