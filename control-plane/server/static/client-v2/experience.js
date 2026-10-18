/* V2 experience adapter. Cookies are HttpOnly; shared API secrets never enter JS. */
(function (root) {
  const enabled = root.document?.body?.dataset.clientVersion === "2";
  function apiUrl(path, origin = root.location.origin) {
    const url = new URL(path, origin);
    if (url.origin !== origin || !url.pathname.startsWith("/api/v1/") || url.search || url.hash) throw Error("Invalid client API path");
    return `/client-v2/api/${url.pathname.slice("/api/v1/".length)}`;
  }
  function coverOffset(index, selected) { return Math.max(-3, Math.min(3, index - selected)); }
  async function waitForSession(initial, load, pause, timeout = 45000, clock = () => Date.now()) {
    const deadline = clock() + timeout;
    let session = initial;
    while (true) {
      if (["launch_failed", "allocation_failed", "runtime_lost", "stopped", "stopping", "terminated", "unhealthy"].includes(session.state)) {
        throw Error(session.failure_reason || "The game could not start. Please choose another game.");
      }
      if (session.state === "active" && session.connection_grant?.webrtc_signaling?.offer_path) return session;
      if (clock() >= deadline) throw Error("The game is taking too long to start. Check Active sessions before starting another.");
      await pause(500);
      session = await load(initial.id);
    }
  }

  const api = { apiUrl, coverOffset, waitForSession };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  if (!enabled) return;
  root.FourPlayV2 = api;
  const launchIndicator = root.document.createElement("div");
  launchIndicator.className = "v2-launch-indicator";
  launchIndicator.setAttribute("role", "status");
  launchIndicator.textContent = "Starting your game — getting video and controls ready…";
  launchIndicator.hidden = true;
  root.document.body.append(launchIndicator);
  let launching = false;
  Object.defineProperty(api, "launching", {
    get: () => launching,
    set: (value) => {
      launching = Boolean(value); launchIndicator.hidden = !launching;
      root.document.body.classList.toggle("v2-launching", launching);
    },
  });
  api.headers = { "X-4Play-Client": "v2" };
  api.mediaConfiguration = async () => {
    const response = await root.fetch("/client-v2/webrtc-config", { credentials: "same-origin", cache: "no-store", headers: api.headers });
    const result = await response.json();
    if (!response.ok) {
      if (response.status === 401 && result.code === "tester_access_required") root.location.replace("/client-v2/tester-access");
      throw Error(result.message || "Media configuration unavailable.");
    }
    return result;
  };
  api.artworkEnvironment = {
    URL: root.URL, AbortController: root.AbortController,
    fetch: (path, options) => root.fetch(apiUrl(path), { ...options, credentials: "same-origin" }),
  };
  api.bootstrap = async () => {
    const response = await root.fetch("/client-v2/bootstrap", { method: "POST", credentials: "same-origin", headers: { ...api.headers, "Content-Type": "application/json" }, body: "{}" });
    if (!response.ok) {
      const error = await response.json().catch(() => ({}));
      if (response.status === 401 && error.code === "tester_access_required") root.location.replace("/client-v2/tester-access");
      throw Error(error.message || "This seat could not connect. Please ask the site operator.");
    }
    return response.json();
  };

  const loaders = new Map();
  api.updateCovers = (selection, cards) => {
    const live = new Set(cards);
    for (const [card, loader] of loaders) {
      if (!live.has(card) || Math.abs(Array.from(cards).indexOf(card) - selection.index) > 3) {
        loader.clear(); loaders.delete(card); card.querySelector(".cover-art")?.remove();
      }
    }
    cards.forEach((card, index) => {
      const offset = coverOffset(index, selection.index);
      card.style.setProperty("--flow-offset", offset);
      card.style.setProperty("--flow-turn", `${offset === 0 ? 0 : offset < 0 ? 26 : -26}deg`);
      card.style.setProperty("--flow-depth", `${offset === 0 ? 45 : -Math.abs(offset) * 35}px`);
      card.style.zIndex = String(10 - Math.abs(offset));
      card.style.setProperty("--flow-distance", Math.abs(offset));
      if (Math.abs(index - selection.index) > 3 || loaders.has(card)) return;
      const image = root.document.createElement("img");
      image.className = "cover-art"; image.alt = ""; image.hidden = true;
      card.prepend(image);
      const loader = new root.FourPlayCarousel.ArtworkLoader(api.artworkEnvironment);
      loaders.set(card, loader);
      loader.load(selection.games[index], api.headers, (url) => {
        image.hidden = !url; if (url) image.src = url; else image.removeAttribute("src");
      }, "cover");
      image.onerror = () => { image.hidden = true; };
    });
  };
  api.bindRail = (rail, select) => {
    const navigation = root.FourPlayCarousel.bindRail(rail, select, root);
    api.centerCard = (card, behavior) => navigation.center(card, behavior);
  };
})(typeof window === "undefined" ? globalThis : window);
