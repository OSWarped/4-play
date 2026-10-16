/* Minimal presentation only; V2 security/media and V3 gallery logic are reused. */
(function (root) {
  if (root.document?.body?.dataset.clientSkin !== "minimal") return;
  const document = root.document, adapter = root.FourPlayV2;
  const topbar = document.querySelector(".topbar"), search = document.querySelector("#game-filter");
  const legacy = document.createElement("div"); legacy.hidden = true;
  legacy.append(...Array.from(topbar.childNodes));
  const toolbar = document.createElement("div"); toolbar.className = "v4-toolbar";
  toolbar.setAttribute("role", "search"); toolbar.setAttribute("aria-label", "Browse games");
  search.placeholder = "Search games";
  const filters = document.createElement("details"); filters.className = "v4-filters";
  const summary = document.createElement("summary"); summary.textContent = "Filters";
  const filterBody = document.createElement("div"); filterBody.className = "v4-filter-body";
  const genre = document.querySelector(".v3-filters select");
  const [live, clear] = document.querySelectorAll(".v3-filters button");
  live.textContent = "Live"; live.setAttribute("aria-label", "Show games playing now");
  const label = document.createElement("label"); label.textContent = "Genre";
  genre.id = "v4-genre"; label.htmlFor = genre.id;
  filterBody.append(label, genre, clear); filters.append(summary, filterBody);
  toolbar.append(search, live, filters); topbar.append(toolbar, legacy);
  const connection = document.createElement("div"); connection.className = "v4-connection";
  connection.setAttribute("role", "alert"); connection.hidden = true; topbar.after(connection);
  genre.addEventListener("change", () => {
    summary.textContent = genre.value ? "Filters •" : "Filters";
    filters.open = false;
  });
  clear.addEventListener("click", () => { summary.textContent = "Filters"; filters.open = false; });
  document.addEventListener("pointerdown", event => { if (!filters.contains(event.target)) filters.open = false; });
  filters.addEventListener("keydown", event => { if (event.key === "Escape") { filters.open = false; summary.focus(); } });

  const panel = document.createElement("dialog"); panel.className = "v4-game-panel";
  const close = document.createElement("button"); close.type = "button"; close.className = "v4-close";
  close.textContent = "×"; close.setAttribute("aria-label", "Close game details");
  const details = document.querySelector("#details"), oldDetails = details.parentElement;
  panel.append(close, document.querySelector(".featured-game"), details); oldDetails.remove(); document.body.append(panel);
  document.querySelector(".v3-play-dock").hidden = true;
  const empty = document.createElement("div"); empty.className = "v4-empty"; empty.hidden = true;
  const emptyText = document.createElement("p"); emptyText.textContent = "No games match your filters."; emptyText.setAttribute("role", "status");
  const reset = document.createElement("button"); reset.type = "button"; reset.textContent = "Clear filters";
  reset.addEventListener("click", () => clear.click()); empty.append(emptyText, reset);
  document.querySelector(".library-panel").append(empty);
  const photo = document.querySelector("#featured-image"), art = document.querySelector("#featured-art");
  new root.MutationObserver(() => { art.hidden = photo.hidden; }).observe(photo, { attributes: true, attributeFilter: ["hidden"] });
  let selected = null, opener = null, browseY = 0, playing = false;
  const restore = () => {
    if (opener?.isConnected) opener.focus({ preventScroll: true });
    root.scrollTo({ top: browseY, behavior: "instant" });
  };
  panel.addEventListener("close", () => {
    document.body.classList.remove("v4-panel-open");
    if (!playing) restore();
  });
  close.addEventListener("click", () => panel.close());
  panel.addEventListener("click", event => {
    if (event.target !== panel) return;
    const rect = panel.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) panel.close();
  });
  const update = adapter.updateCovers;
  let sessions = [];
  adapter.updateCovers = (selection, cards) => {
    update(selection, cards); selected = selection.selected;
    empty.hidden = selection.games.length > 0;
    document.querySelector("#featured-description").hidden = !selected?.metadata?.description;
    for (const card of cards) {
      card.dataset.live = String(sessions.some(session => session.state === "active" && session.game_id === card.dataset.gameId));
    }
  };
  document.querySelector("#games").addEventListener("click", event => {
    const card = event.target.closest(".game-cover");
    if (!card || !selected) return;
    opener = card; browseY = root.scrollY; playing = false; filters.open = false;
    panel.setAttribute("aria-label", root.FourPlayCarousel.displayTitle(selected));
    root.renderGameDetails(selected);
    if (!panel.open) panel.showModal();
    document.body.classList.add("v4-panel-open");
    close.focus({ preventScroll: true });
  });
  root.FourPlayV4 = {
    setCatalog(_games, value) { sessions = value; },
    showConnectionError(message) {
      connection.textContent = message; connection.hidden = !message;
      if (message) {
        const retry = document.createElement("button"); retry.type = "button"; retry.textContent = "Try again";
        retry.addEventListener("click", () => root.location.reload()); connection.append(retry);
      }
    },
    closeForPlay() { playing = true; if (panel.open) panel.close(); document.body.classList.remove("v4-panel-open"); },
    restoreBrowse() { playing = false; restore(); },
  };
})(typeof window === "undefined" ? globalThis : window);
