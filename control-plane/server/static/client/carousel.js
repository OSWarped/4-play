/* Library-only navigation. No timers, gameplay input, session grants or launches. */
(function (root) {
  class GameSelection {
    constructor() { this.games = []; this.selectedId = null; }
    update(games, query = "") {
      const needle = query.trim().toLowerCase();
      this.games = games.filter((game) => [game.display_name, game.id,
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

  function artworkPaths(game) {
    const metadata = game?.metadata || {};
    return [...new Set([metadata.screenshot_path, metadata.artwork_path,
      metadata.marquee_path, metadata.logo_path].filter((path) => typeof path === "string" && path))];
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
    async load(game, headers, apply) {
      this.clear();
      const version = this.version;
      const controller = this.controller = new this.env.AbortController();
      apply(null);
      for (const path of artworkPaths(game)) {
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

  const api = { GameSelection, artworkPaths, assetUrl, ArtworkLoader };
  root.FourPlayCarousel = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
