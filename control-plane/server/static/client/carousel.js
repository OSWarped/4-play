/* Library-only navigation. No timers, gameplay input, session grants or launches. */
(function (root) {
  class GameSelection {
    constructor() { this.games = []; this.selectedId = null; }
    update(games, query = "") {
      const needle = query.trim().toLowerCase();
      this.games = games.filter((game) => [game.display_name, game.metadata?.sort_title, game.id,
        game.metadata?.genre, game.metadata?.manufacturer, game.metadata?.release_year]
        .filter(Boolean).join(" ").toLowerCase().includes(needle));
      if (!this.games.some((game) => game.id === this.selectedId)) {
        this.selectedId = this.games[0]?.id ?? null;
      }
      return this.selected;
    }
    get index() { return this.games.findIndex((game) => game.id === this.selectedId); }
    get selected() { return this.games[this.index] ?? null; }
    select(id) {
      if (this.games.some((game) => game.id === id)) this.selectedId = id;
      return this.selected;
    }
    move(delta) {
      if (this.games.length) {
        this.selectedId = this.games[(this.index + delta + this.games.length) % this.games.length].id;
      }
      return this.selected;
    }
  }

  function displayTitle(game) {
    return [game?.metadata?.sort_title, game?.display_name, game?.game_id, game?.id]
      .find((value) => typeof value === "string" && value.trim())?.trim() || "Untitled game";
  }

  function artworkPaths(game, role = "scene") {
    const metadata = game?.metadata || {};
    const paths = role === "logo" ? [metadata.logo_path] : role === "marquee" ? [metadata.marquee_path]
      : role === "cover" ? [metadata.artwork_path, metadata.screenshot_path, metadata.marquee_path, metadata.logo_path]
      : [metadata.screenshot_path, metadata.artwork_path, metadata.marquee_path, metadata.logo_path];
    return [...new Set(paths.filter((path) => typeof path === "string" && path))];
  }

  // .game-rail is positioned: card offsets are in its untransformed scroll space.
  function cardCenter(card) { return card.offsetLeft + card.offsetWidth / 2; }
  function centeredLeft(rail, card) { return cardCenter(card) - rail.clientWidth / 2; }
  function bindRail(rail, select, env = root) {
    let timer, automatic = false, lastCard = null, lastWidth = 0;
    const cancel = () => { automatic = false; env.clearTimeout(timer); };
    for (const event of ["pointerdown", "touchstart", "wheel"]) rail.addEventListener(event, cancel, { passive: true });
    rail.addEventListener("scroll", () => {
      env.clearTimeout(timer);
      timer = env.setTimeout(() => {
        if (automatic || env.document.body.classList.contains("browser-playing")) return;
        const center = rail.scrollLeft + rail.clientWidth / 2;
        const closest = Array.from(rail.querySelectorAll(".game-cover")).reduce((best, card) => {
          const distance = Math.abs(cardCenter(card) - center);
          return !best || distance < best.distance ? { card, distance } : best;
        }, null);
        if (closest && closest.card.getAttribute("aria-pressed") !== "true") select(closest.card.dataset.gameId);
      }, 180);
    }, { passive: true });
    const navigation = { center(card, behavior) {
      if (card === lastCard && rail.clientWidth === lastWidth) return;
      lastCard = card; lastWidth = rail.clientWidth;
      automatic = true;
      env.clearTimeout(timer);
      const left = centeredLeft(rail, card);
      // Wrap/search jumps should not sweep through dozens of unrelated covers.
      rail.scrollTo({ left, behavior: Math.abs(left - rail.scrollLeft) > rail.clientWidth * 2 ? "instant" : behavior });
    } };
    if (env.ResizeObserver) {
      new env.ResizeObserver(() => {
        if (lastCard && rail.clientWidth > 0 && !env.document.body.classList.contains("browser-playing")) {
          navigation.center(lastCard, "instant");
        }
      }).observe(rail);
    }
    return navigation;
  }

  function assetUrl(path) {
    // Accept only relative catalog assets, never arbitrary URLs or traversal.
    const parts = path.split("/");
    if (parts.some((part) => !part || part === "." || part === "..") || /[:\\\x00-\x1f]/.test(path)) {
      throw new Error("Invalid catalog artwork path");
    }
    return `/api/v1/assets/${parts.map(encodeURIComponent).join("/")}`;
  }

  class ArtworkLoader {
    constructor(env = root) { this.env = env; this.version = 0; this.url = null; this.controller = null; }
    clear() {
      this.version++;
      this.controller?.abort();
      this.controller = null;
      if (this.url) this.env.URL.revokeObjectURL(this.url);
      this.url = null;
    }
    async load(game, headers, apply, role = "scene") {
      this.clear();
      const version = this.version;
      const controller = this.controller = new this.env.AbortController();
      apply(null);
      for (const path of artworkPaths(game, role)) {
        try {
          const response = await this.env.fetch(assetUrl(path), { headers, signal: controller.signal });
          if (version !== this.version || controller.signal.aborted) return;
          if (!response.ok) continue;
          const blob = await response.blob();
          if (version !== this.version) return;
          if (!blob.type.startsWith("image/") || blob.size > 16 * 1024 * 1024) continue;
          this.url = this.env.URL.createObjectURL(blob);
          apply(this.url);
          return;
        } catch (error) {
          if (controller.signal.aborted || version !== this.version) return;
          // Missing/invalid artwork must never block browsing or launching.
        }
      }
    }
  }

  const api = { GameSelection, displayTitle, artworkPaths, assetUrl, ArtworkLoader, cardCenter, centeredLeft, bindRail };
  root.FourPlayCarousel = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
