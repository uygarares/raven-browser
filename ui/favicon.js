// Site logos (favicons) for tabs, bookmarks, history and the new tab page.
//
// First the site's own /favicon.ico, then DuckDuckGo's icon service (which does
// not track), and if both fail, the first letter of the site's name.

const RavenIcons = (() => {
  const failed = new Set();
  /// Logo that worked for each site, so redrawing the tabs does not flicker.
  const found = new Map();

  function hostOf(url) {
    try {
      const parsed = new URL(url);
      return /^https?:$/.test(parsed.protocol) ? parsed.hostname : "";
    } catch {
      return "";
    }
  }

  function letter(url, fallback) {
    const host = hostOf(url).replace(/^www\./, "");
    return (host || fallback || "?").charAt(0).toUpperCase();
  }

  /// Fills `box` with the site's logo, or its first letter while none loads.
  function fill(box, url, fallbackText) {
    box.textContent = letter(url, fallbackText);
    const host = hostOf(url);
    if (!host) return;
    if (found.has(host)) {
      show(box, found.get(host));
      return;
    }
    const sources = [
      `https://${host}/favicon.ico`,
      `https://icons.duckduckgo.com/ip3/${host}.ico`,
    ].filter((source) => !failed.has(source));
    tryNext(box, host, sources);
  }

  function show(box, source) {
    const img = new Image();
    img.alt = "";
    img.referrerPolicy = "no-referrer";
    img.src = source;
    box.textContent = "";
    box.classList.add("has-logo");
    box.append(img);
  }

  function tryNext(box, host, sources) {
    const source = sources.shift();
    if (!source) return;
    const img = new Image();
    img.alt = "";
    img.decoding = "async";
    img.referrerPolicy = "no-referrer";
    img.onload = () => {
      // Some sites answer with a 1x1 placeholder: treat that as no logo.
      if (img.naturalWidth < 8) {
        failed.add(source);
        tryNext(box, host, sources);
        return;
      }
      found.set(host, source);
      box.textContent = "";
      box.classList.add("has-logo");
      box.append(img);
    };
    img.onerror = () => {
      failed.add(source);
      tryNext(box, host, sources);
    };
    img.src = source;
  }

  return { fill, hostOf, letter };
})();
