/* V3 is a catalog layout adapter, not another authentication/media stack. */
(function (root) {
  function genres(games) {
    return [...new Set(games.map(game => game.metadata?.genre?.trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));
  }
  function filterGames(games, genre = "", liveOnly = false, sessions = []) {
    const active = new Set(sessions.filter(session => session.state === "active").map(session => session.game_id));
    return games.filter(game => (!genre || game.metadata?.genre?.trim() === genre) && (!liveOnly || active.has(game.id)));
  }
  const exports = { genres, filterGames };
  if (typeof module !== "undefined" && module.exports) module.exports = exports;
  if (root.document?.body?.dataset.clientLayout !== "grid") return;
  const document = root.document, adapter = root.FourPlayV2;
  document.querySelector(".topbar").append(document.querySelector(".hero-actions"));
  const library = document.querySelector(".library-panel");
  const controls = document.createElement("div"); controls.className = "v3-filters";
  const select = document.createElement("select"); select.setAttribute("aria-label", "Filter by genre");
  const live = document.createElement("button"); live.type = "button"; live.textContent = "Playing now"; live.setAttribute("aria-pressed", "false");
  const clear = document.createElement("button"); clear.type = "button"; clear.textContent = "Clear filters";
  controls.append(select, live, clear);
  library.querySelector(".section-heading").after(controls);
  let selectedGenre = "", liveOnly = false, genreKey = null;
  select.addEventListener("change", () => { selectedGenre = select.value; root.renderGames(); });
  live.addEventListener("click", () => {
    liveOnly = !liveOnly; live.setAttribute("aria-pressed", String(liveOnly)); root.renderGames();
  });
  clear.addEventListener("click", () => {
    selectedGenre = ""; liveOnly = false; select.value = ""; live.setAttribute("aria-pressed", "false");
    document.querySelector("#game-filter").value = ""; root.renderGames();
  });
  root.FourPlayV3 = {
    updateFilters(games) {
      const values = genres(games), key = JSON.stringify(values);
      if (key === genreKey) return;
      genreKey = key;
      if (!values.includes(selectedGenre)) selectedGenre = "";
      select.replaceChildren(...["", ...values].map(value => {
        const option = document.createElement("option"); option.value = value; option.textContent = value || "All genres"; return option;
      }));
      select.value = selectedGenre;
    },
    filterGames: (games, sessions) => filterGames(games, selectedGenre, liveOnly, sessions),
  };
  document.querySelector("#game-carousel").removeAttribute("aria-roledescription");
  document.querySelector("#game-carousel").setAttribute("aria-label", "Browse the game catalog");
  document.querySelector(".carousel-hint").textContent = "Choose a game to preview it. Use arrow keys to browse; start, join or watch from the selected game actions.";
  document.querySelector(".library-panel h2").textContent = "Find your next favorite";
  const dock = document.createElement("div"); dock.className = "v3-play-dock";
  dock.setAttribute("aria-label", "Selected game actions");
  dock.append(document.querySelector(".featured-copy .form-actions")); library.append(dock);
  const loaders = new Map(), observed = new Map();
  const load = card => {
    const game = observed.get(card);
    if (!game || loaders.has(card)) return;
    const image = document.createElement("img"); image.className = "cover-art"; image.alt = ""; image.hidden = true; card.prepend(image);
    const loader = new root.FourPlayCarousel.ArtworkLoader(adapter.artworkEnvironment); loaders.set(card, loader);
    loader.load(game, adapter.headers, url => { image.hidden = !url; if (url) image.src = url; else image.removeAttribute("src"); }, "cover");
    image.onerror = () => { image.hidden = true; };
  };
  const observer = root.IntersectionObserver ? new root.IntersectionObserver(entries => {
    for (const entry of entries) if (entry.isIntersecting) { load(entry.target); observer.unobserve(entry.target); }
  }, { rootMargin: "200px" }) : null;
  adapter.updateCovers = (selection, cards) => {
    const current = new Set(cards);
    for (const card of observed.keys()) if (!current.has(card)) {
      observer?.unobserve(card); loaders.get(card)?.clear(); loaders.delete(card); observed.delete(card);
    }
    cards.forEach((card, index) => {
      if (observed.has(card)) return;
      observed.set(card, selection.games[index]);
      if (observer) observer.observe(card); else load(card);
    });
    const game = selection.selected;
    document.querySelector("#featured-start").setAttribute("aria-label", game ? `Start ${root.FourPlayCarousel.displayTitle(game)}` : "Start game");
  };
  // No horizontal scroll-selection feedback in a grid. Keyboard focus alone may scroll.
  adapter.bindRail = rail => {
    rail.addEventListener("keydown", event => {
      if (!["ArrowUp", "ArrowDown"].includes(event.key) || !event.target.closest(".game-cover")) return;
      event.preventDefault();
      const cards = Array.from(rail.querySelectorAll(".game-cover"));
      const columns = cards.filter(card => card.offsetTop === cards[0]?.offsetTop).length || 1;
      root.moveFeaturedGame(event.key === "ArrowUp" ? -columns : columns, true);
    });
  };
  adapter.centerCard = (card, behavior) => {
    if (document.activeElement === card) card.scrollIntoView({ block: "nearest", behavior });
  };
  root.addEventListener("pagehide", () => {
    for (const card of observed.keys()) {
      observer?.unobserve(card); loaders.get(card)?.clear(); card.querySelector(".cover-art")?.remove();
    }
    loaders.clear(); observed.clear();
  });
})(typeof window === "undefined" ? globalThis : window);
