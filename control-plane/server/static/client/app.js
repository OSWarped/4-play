const tokenInput = document.querySelector("#api-token");
const seatInput = document.querySelector("#seat-id");
const destinationInput = document.querySelector("#destination-ip");
const destinationWarning = document.querySelector("#destination-warning");
const saveSettingsButton = document.querySelector("#save-settings");
const refreshButton = document.querySelector("#refresh");
const gameFilter = document.querySelector("#game-filter");
const browserPlayer = document.querySelector("#browser-player");
const browserMediaVideo = document.querySelector("#browser-media-video");
const browserMediaState = document.querySelector("#browser-media-state");
const stopBrowserMediaButton = document.querySelector("#stop-browser-media");
const reconnectBrowserMediaButton = document.querySelector("#reconnect-browser-media");
const browserReceiverStats = document.querySelector("#browser-receiver-stats");
const librarySelection = new FourPlayCarousel.GameSelection();
const libraryArtwork = new FourPlayCarousel.ArtworkLoader(window.FourPlayV2?.artworkEnvironment);
const libraryLogo = new FourPlayCarousel.ArtworkLoader(window.FourPlayV2?.artworkEnvironment);
const libraryMarquee = new FourPlayCarousel.ArtworkLoader(window.FourPlayV2?.artworkEnvironment);
let libraryArtworkKey = null;
let libraryCardsKey = null;
let libraryLaunchPending = false;

const state = {
  token: window.FourPlayV2 ? "" : localStorage.getItem("fourplay.clientToken") || "",
  seatId: localStorage.getItem("fourplay.seatId") || "windows-seat-1",
  destinationIp: localStorage.getItem("fourplay.destinationIp") || "",
  lastRuntime: window.FourPlayV2 ? null : loadLastRuntime(),
  browserMediaStatus: null,
  browserMediaPeer: null,
  browserMediaStream: null,
  browserInput: null,
  browserInputLeaseTimer: null,
  browserMediaContext: null,
  browserExitPending: false,
  browserMediaMonitor: null,
  browserMediaRetry: null,
  browserMediaNegotiating: false,
  spectatorOverlay: null,
  spectatorSlotsTimer: null,
  spectatorSlotsLoading: false,
  browserJoinPending: false,
  games: [],
  sessions: [],
};

const clientApi = {
  async loadCatalog() {
    return fetchJson("/api/v1/games");
  },
  async loadActiveSessions() {
    return fetchJson("/api/v1/active-sessions");
  },
  async loadBrowserMediaStatus() {
    return fetchJson("/api/v1/experimental/webrtc/status");
  },
  async startGame(gameId, seatId, destinationAddress) {
    return postJson("/api/v1/sessions", {
      game_id: gameId,
      seat_id: seatId,
      destination_address: destinationAddress,
    });
  },
  async reservePlayerSlot(sessionId, playerNumber, seatId) {
    return postJson(`/api/v1/sessions/${sessionId}/player-slots/${playerNumber}/reserve`, {
      seat_id: seatId,
    });
  },
  async connectPlayerSlot(sessionId, playerNumber, seatId) {
    return postJson(`/api/v1/sessions/${sessionId}/player-slots/${playerNumber}/connect`, {
      seat_id: seatId,
    });
  },
  async disconnectPlayerSlot(sessionId, playerNumber, seatId) {
    return postJson(`/api/v1/sessions/${sessionId}/player-slots/${playerNumber}/disconnect`, {
      seat_id: seatId,
    });
  },
  async releasePlayerSlot(sessionId, playerNumber, seatId) {
    return postJson(`/api/v1/sessions/${sessionId}/player-slots/${playerNumber}/release`, { seat_id: seatId });
  },
  async spectate(sessionId, seatId, destinationAddress) {
    return postJson(`/api/v1/sessions/${sessionId}/spectators`, {
      seat_id: seatId,
      destination_address: destinationAddress,
    });
  },
  async releaseSpectatorGrant(sessionId, grantId, seatId) {
    return postJson(`/api/v1/sessions/${sessionId}/spectators/${grantId}`, { seat_id: seatId }, "DELETE");
  },
};

tokenInput.value = state.token;
seatInput.value = state.seatId;
destinationInput.value = state.destinationIp;
renderDestinationWarning();
document.querySelector("#seat-settings").open = !state.token;

saveSettingsButton.addEventListener("click", () => {
  state.token = tokenInput.value.trim();
  state.seatId = seatInput.value.trim() || "windows-seat-1";
  state.destinationIp = destinationInput.value.trim();
  localStorage.setItem("fourplay.clientToken", state.token);
  localStorage.setItem("fourplay.seatId", state.seatId);
  localStorage.setItem("fourplay.destinationIp", state.destinationIp);
  renderDestinationWarning();
  document.querySelector("#seat-settings").open = false;
  refresh();
});

refreshButton.addEventListener("click", refresh);
destinationInput.addEventListener("input", () => {
  state.destinationIp = destinationInput.value.trim();
  renderDestinationWarning();
});
gameFilter.addEventListener("input", renderGames);
document.querySelector("#game-previous").addEventListener("click", () => moveFeaturedGame(-1));
document.querySelector("#game-next").addEventListener("click", () => moveFeaturedGame(1));
document.querySelector("#featured-details").addEventListener("click", () => {
  if (librarySelection.selected) {
    renderGameDetails(librarySelection.selected);
    document.querySelector("#details").scrollIntoView({ behavior: "smooth", block: "start" });
  }
});
document.querySelector("#featured-start").addEventListener("click", async () => {
  const game = librarySelection.selected;
  if (!game || libraryLaunchPending) return;
  libraryLaunchPending = true;
  document.querySelector("#featured-start").disabled = true;
  renderGameDetails(game);
  document.querySelector("#details").scrollIntoView({ behavior: "smooth", block: "start" });
  try { await startGame(game); }
  finally {
    libraryLaunchPending = false;
    document.querySelector("#featured-start").disabled = !librarySelection.selected;
  }
});
document.querySelector("#games").addEventListener("keydown", (event) => {
  if (!event.target.closest(".game-cover")) return;
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  if (event.key === "Home" || event.key === "End") {
    librarySelection.select(librarySelection.games[event.key === "Home" ? 0 : librarySelection.games.length - 1]?.id);
    updateFeaturedGame(true);
  } else moveFeaturedGame(event.key === "ArrowLeft" ? -1 : 1, true);
});
window.addEventListener("pagehide", () => libraryArtwork.clear());
window.addEventListener("pageshow", (event) => {
  if (event.persisted) { libraryArtworkKey = null; updateFeaturedGame(); }
});
document.querySelector("#browse-games").addEventListener("click", () => {
  document.querySelector("#games").scrollIntoView({ behavior: "smooth", block: "start" });
});
document.querySelector("#browse-sessions").addEventListener("click", () => {
  document.querySelector("#sessions").scrollIntoView({ behavior: "smooth", block: "start" });
});
stopBrowserMediaButton.addEventListener("click", leaveBrowserGame);
reconnectBrowserMediaButton.addEventListener("click", () => {
  if (state.browserMediaRetry) startBrowserMedia({ dataset: { ...state.browserMediaRetry } });
});
document.querySelector("#resume-browser-media").addEventListener("click", resumeBrowserMedia);
FourPlayBrowserInput.bindEscape(browserPlayer, leaveBrowserGame, () => state.browserInput?.release());
document.querySelector("#fullscreen-browser-media").addEventListener("click", async () => {
  try {
    await browserPlayer.requestFullscreen();
    browserMediaVideo.focus();
  } catch (error) { setBrowserMediaState(`Fullscreen unavailable: ${error.message}`); }
});
browserMediaVideo.addEventListener("click", () => browserMediaVideo.focus());
document.querySelector("#spectator-fullscreen").addEventListener("click", () => document.querySelector("#fullscreen-browser-media").click());
document.querySelector("#spectator-reconnect").addEventListener("click", () => reconnectBrowserMediaButton.click());
document.querySelector("#spectator-leave").addEventListener("click", leaveBrowserGame);
window.addEventListener("pagehide", stopBrowserMedia);

function authHeaders() {
  if (window.FourPlayV2) return window.FourPlayV2.headers;
  if (!state.token) return {};
  return { Authorization: `Bearer ${state.token}` };
}

async function fetchJson(path) {
  const response = await fetch(window.FourPlayV2 ? window.FourPlayV2.apiUrl(path) : path, { headers: authHeaders() });
  if (!response.ok) {
    const body = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${body}`);
  }
  return response.json();
}

async function postJson(path, body, method = "POST") {
  const response = await fetch(window.FourPlayV2 ? window.FourPlayV2.apiUrl(path) : path, {
    method,
    headers: {
      ...authHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const responseBody = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${responseBody}`);
  }
  return response.json();
}

async function refresh() {
  setText("#primary-title", "Loading 4-Play…");
  setText("#primary-detail", "Fetching games and active sessions.");
  try {
    const [games, sessions, browserMediaStatus] = await Promise.all([
      clientApi.loadCatalog(),
      clientApi.loadActiveSessions(),
      clientApi.loadBrowserMediaStatus(),
    ]);
    state.games = games.games || [];
    state.sessions = sessions.sessions || [];
    state.browserMediaStatus = browserMediaStatus;
    renderHome();
    renderGames();
    renderSessions();
  } catch (error) {
    setText("#primary-title", "Cannot reach 4-Play");
    setText("#primary-detail", String(error));
    renderDetails(`
      <div class="item">
        <h3>Connection problem</h3>
        <p class="muted">${window.FourPlayV2 ? "Reload this page or ask the site operator to check the connection." : "Check the seat token, network, and control-plane service."}</p>
      </div>
    `);
  }
}

function renderHome() {
  const ownDisconnected = findOwnDisconnectedSlot();
  const openSession = state.sessions.find((session) =>
    (session.player_slots || []).some((slot) => slot.state === "open"),
  );
  if (ownDisconnected) {
    setText("#primary-title", `Rejoin ${ownDisconnected.displayName}`);
    setText("#primary-detail", window.FourPlayV2 ? "Your player slot is waiting. Rejoin from Active sessions." : `Seat ${state.seatId} has a disconnected player slot waiting.`);
  } else if (openSession) {
    setText("#primary-title", `Join ${sessionTitle(openSession)}`);
    setText("#primary-detail", "An active game has an open player slot.");
  } else if (state.sessions.length) {
    setText("#primary-title", "Spectate active games");
    setText("#primary-detail", "Games are running, but no open player slot is currently advertised.");
  } else {
    setText("#primary-title", "Choose a game");
    setText("#primary-detail", `${state.games.length} games are available.`);
  }
  renderBrowserMediaStatus();
  renderLastRuntimeHint();
}

function renderBrowserMediaStatus() {
  const container = document.querySelector("#browser-media-status");
  if (!container) return;
  const status = state.browserMediaStatus;
  if (!status) {
    container.innerHTML = "";
    return;
  }
  const hostCount = status.web_rtc_runtime_hosts?.length || 0;
  const ready = Boolean(status.session_publisher_available);
  const gated = !ready && hostCount > 0;
  const spikeOnly = !ready && Boolean(status.test_pattern_proxy_configured);
  const tone = ready ? "good" : gated || spikeOnly ? "warn" : "";
  const label = ready
    ? "Browser media ready"
    : gated
      ? "WebRTC host gated"
      : spikeOnly
        ? "WebRTC spike only"
        : "FFplay fallback";
  const detail = ready
    ? `${hostCount} runtime host${hostCount === 1 ? "" : "s"} can publish browser-native media.`
    : gated
      ? `${hostCount} runtime host${hostCount === 1 ? "" : "s"} advertise WebRTC prerequisites, but session offer routing is still disabled.`
    : spikeOnly
      ? "Test-pattern WebRTC is available; gameplay still uses the proven FFplay path."
      : "Gameplay media uses the proven UDP/FFplay path until WebRTC publishers are enabled.";
  container.innerHTML = `
    <div class="browser-media-status">
      <span class="pill ${tone}">${escapeHtml(label)}</span>
      <span class="muted">${escapeHtml(detail)}</span>
    </div>
  `;
}

function findOwnDisconnectedSlot() {
  for (const session of state.sessions) {
    const slot = (session.player_slots || []).find(
      (candidate) => candidate.seat_id === state.seatId && candidate.state === "disconnected",
    );
    if (slot) {
      return { session, slot, displayName: sessionTitle(session) };
    }
  }
  return null;
}

function renderGames() {
  librarySelection.update(state.games, gameFilter.value);
  const key = JSON.stringify(librarySelection.games);
  if (key !== libraryCardsKey) {
    libraryCardsKey = key;
    replaceChildren(document.querySelector("#games"), librarySelection.games.map(renderGame));
  }
  updateFeaturedGame();
}

function renderGame(game) {
  const metadata = game.metadata || {};
  const element = document.createElement("button");
  element.type = "button";
  element.className = "game-cover";
  element.dataset.gameId = game.id;
  element.setAttribute("aria-label", `Feature ${FourPlayCarousel.displayTitle(game)}`);
  element.innerHTML = `
    <span class="cover-kicker">${escapeHtml(metadata.genre || "Arcade")}</span>
    <span class="cover-name">${escapeHtml(FourPlayCarousel.displayTitle(game))}</span>
    <span class="cover-footer">${escapeHtml(String(metadata.release_year || "CLASSIC"))} <span>${escapeHtml(String(metadata.player_count || game.availability?.[0]?.profile?.max_players || "?"))}P</span></span>
  `;
  element.addEventListener("click", () => {
    librarySelection.select(game.id);
    updateFeaturedGame();
  });
  return element;
}

function moveFeaturedGame(delta, focus = false) {
  librarySelection.move(delta);
  updateFeaturedGame(focus);
}

function updateFeaturedGame(focus = false) {
  const game = librarySelection.selected;
  const metadata = game?.metadata || {};
  const count = librarySelection.games.length;
  setText("#carousel-status", game ? `${librarySelection.index + 1} / ${count} games` : "No matching games. Try another search.");
  setText("#featured-title", game ? FourPlayCarousel.displayTitle(game) : "No games to show");
  setText("#featured-genre", metadata.genre || "ARCADE LIBRARY");
  setText("#featured-meta", game ? [metadata.release_year, metadata.manufacturer,
    `${metadata.player_count || game.availability?.[0]?.profile?.max_players || "?"} players`].filter(Boolean).join(" · ") : "");
  setText("#featured-description", game ? metadata.description || "An arcade classic. Open details for controls and session options." : "Clear the filter or refresh your library.");
  const running = state.sessions.filter((session) => session.game_id === game?.id).length;
  setText("#featured-running", running ? `${running} running session${running === 1 ? "" : "s"} · Open details to join or spectate` : "Start a new session, or see what others are playing alongside the library.");
  document.querySelector("#featured-start").disabled = !game || libraryLaunchPending;
  document.querySelector("#featured-details").disabled = !game;
  for (const id of ["#game-previous", "#game-next"]) document.querySelector(id).disabled = count < 2;
  const covers = document.querySelectorAll(".game-cover");
  window.FourPlayV2?.updateCovers(librarySelection, covers);
  for (const card of covers) {
    const selected = card.dataset.gameId === game?.id;
    card.setAttribute("aria-pressed", String(selected));
    card.tabIndex = selected ? 0 : -1;
    if (selected) {
      if (focus) card.focus({ preventScroll: true });
      // Scroll the rail only; never pull the page away from gameplay/details.
      const rail = card.parentElement;
      const behavior = window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
      if (window.FourPlayV2?.centerCard) window.FourPlayV2.centerCard(card, behavior);
      else rail.scrollTo({ left: FourPlayCarousel.centeredLeft(rail, card), behavior });
    }
  }
  const artworkKey = JSON.stringify([game?.id, FourPlayCarousel.artworkPaths(game), state.token]);
  if (artworkKey !== libraryArtworkKey) {
    libraryArtworkKey = artworkKey;
    const image = document.querySelector("#featured-image");
    libraryArtwork.load(game, authHeaders(), (url) => {
      image.hidden = !url;
      if (url) image.src = url;
      else image.removeAttribute("src");
      const backdrop = document.querySelector("#library-backdrop");
      backdrop.hidden = !url;
      if (url) backdrop.src = url;
      else backdrop.removeAttribute("src");
    });
    image.onerror = () => { image.hidden = true; };
    FourPlayCarousel.loadBrand(game, authHeaders(), libraryLogo, libraryMarquee,
      document.querySelector("#featured-logo"), document.querySelector("#featured-marquee"),
      document.querySelector("#featured-title"));
  }
}

function renderSessions() {
  replaceChildren(document.querySelector("#sessions"), state.sessions.map(renderSession));
}

function sessionTitle(session) {
  return FourPlayCarousel.displayTitle(state.games.find((game) => game.id === session.game_id) || session);
}

function renderSession(session) {
  const element = item();
  const slots = (session.player_slots || [])
    .map((slot) => {
      const tone = slot.state === "open" ? "good" : slot.state === "disconnected" ? "warn" : "";
      const label = slot.label || `P${slot.player_number}`;
      const owner = window.FourPlayV2 ? slot.seat_id === state.seatId ? " (you)" : "" : slot.seat_id ? ` by ${slot.seat_id}` : "";
      return `<span class="pill ${tone}">${escapeHtml(label)}: ${escapeHtml(slot.state || "open")}${escapeHtml(owner)}</span>`;
    })
    .join("");
  element.classList.add("clickable");
  element.innerHTML = `
    <h3>${escapeHtml(sessionTitle(session))}</h3>
    <span class="pill ${sessionStateTone(session.state)}">${escapeHtml(session.state || "active")}</span>
    ${destinationMismatchBadge(session)}
    <span class="pill">${escapeHtml(String(session.active_spectator_count || 0))} spectators</span>
    <div>${slots}</div>
    ${session.failure_reason ? `<p class="muted">${escapeHtml(session.failure_reason)}</p>` : ""}
  `;
  element.addEventListener("click", () => renderSessionDetails(session));
  return element;
}

function sessionStateTone(sessionState) {
  return ["active", "launching", "allocated"].includes(sessionState) ? "good"
    : ["runtime_lost", "launch_failed", "allocation_failed"].includes(sessionState) ? "bad"
      : ["stop_requested", "stopped"].includes(sessionState) ? "warn"
        : "";
}

function destinationMismatchBadge(session) {
  if (window.FourPlayV2) return "";
  if (!session.destination_address || !state.destinationIp) return "";
  if (session.destination_address === state.destinationIp) return "";
  return `<span class="pill warn">streaming to ${escapeHtml(session.destination_address)}</span>`;
}

function renderGameDetails(game) {
  const running = state.sessions.filter((session) => session.game_id === game.id);
  const metadata = game.metadata || {};
  renderDetails(`
    <div class="item">
      <h3>${escapeHtml(FourPlayCarousel.displayTitle(game))}</h3>
      <span class="pill">${escapeHtml(metadata.genre || "unknown genre")}</span>
      <span class="pill">${escapeHtml(String(metadata.release_year || "unknown year"))}</span>
      <span class="pill">${escapeHtml(metadata.manufacturer || "unknown maker")}</span>
      <p class="muted">${escapeHtml(metadata.description || "No description yet.")}</p>
      <p><strong>Controls:</strong> ${escapeHtml(metadata.control_notes || "No control notes yet.")}</p>
      <div class="form-actions">
        <button type="button" id="start-game">Start game</button>
        ${running.length ? `<button type="button" data-client-action="running">Show running sessions</button>` : ""}
      </div>
      <p class="muted">Start now requests a real session and prepares the local runtime commands for media and input.</p>
      <div id="running-game-sessions" class="stack compact" hidden></div>
      <div id="client-action-result"></div>
    </div>
  `);
  document.querySelector("#start-game").addEventListener("click", () => startGame(game));
  const runningButton = document.querySelector("[data-client-action='running']");
  if (runningButton) {
    runningButton.addEventListener("click", () => renderRunningGameSessions(running));
  }
}

function renderRunningGameSessions(sessions) {
  const container = document.querySelector("#running-game-sessions");
  if (!container) return;
  container.hidden = false;
  replaceChildren(container, sessions.map(renderInlineSession));
}

function renderInlineSession(session) {
  const element = item();
  const openSlots = (session.player_slots || []).filter((slot) => slot.state === "open");
  const ownDisconnected = (session.player_slots || []).filter(
    (slot) => slot.state === "disconnected" && slot.seat_id === state.seatId,
  );
  element.innerHTML = `
    <h4>${escapeHtml(sessionTitle(session))}</h4>
    <span class="pill ${sessionStateTone(session.state)}">${escapeHtml(session.state || "active")}</span>
    ${destinationMismatchBadge(session)}
    <div class="form-actions">
      ${ownDisconnected.map((slot) => `<button type="button" data-inline-rejoin="${escapeHtml(String(slot.player_number))}">Rejoin ${escapeHtml(slot.label || `P${slot.player_number}`)}</button>`).join("")}
      ${openSlots.map((slot) => `<button type="button" data-inline-join="${escapeHtml(String(slot.player_number))}">Join ${escapeHtml(slot.label || `P${slot.player_number}`)}</button>`).join("")}
      <button type="button" data-inline-spectate="true">Spectate</button>
    </div>
  `;
  element.querySelectorAll("[data-inline-join]").forEach((button) => {
    button.addEventListener("click", () =>
      joinPlayerSlot(session, Number(button.dataset.inlineJoin), false),
    );
  });
  element.querySelectorAll("[data-inline-rejoin]").forEach((button) => {
    button.addEventListener("click", () =>
      joinPlayerSlot(session, Number(button.dataset.inlineRejoin), true),
    );
  });
  element
    .querySelector("[data-inline-spectate]")
    .addEventListener("click", () => spectateSession(session));
  return element;
}

async function startGame(game) {
  if (window.FourPlayV2?.launching) return;
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!state.destinationIp) {
    result.innerHTML = `<p class="pill bad">Enter this seat's Media IP before starting a game.</p>`;
    return;
  }
  if (window.FourPlayV2) window.FourPlayV2.launching = true;
  result.innerHTML = `<p class="pill warn">Requesting session...</p>`;
  try {
    const session = await clientApi.startGame(game.id, state.seatId, state.destinationIp);
    rememberRuntime(publicPlayerHandoff(session, 1));
    if (window.FourPlayV2) { await openV2Player(session, 1); await refresh(); return; }
    result.innerHTML = `
      <div class="item">
        <h3>Session requested</h3>
        <span class="pill good">${escapeHtml(session.state || "allocating")}</span>
        <span class="pill">${escapeHtml(session.id)}</span>
        ${runtimePills(session.connection_grant)}
        <p class="muted">For now, use the media command below. Input forwarding requires the packaged client/native handoff so the session token stays hidden.</p>
        ${playerRuntimePanel(session, 1)}
      </div>
    `;
    wireCopyButtons(result);
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
    if (window.FourPlayV2) setText("#primary-detail", error.message || String(error));
  } finally {
    if (window.FourPlayV2) window.FourPlayV2.launching = false;
  }
}

async function openV2Player(session, playerNumber) {
  setText("#primary-title", "Starting your game…");
  setText("#primary-detail", "Getting the screen and controls ready.");
  const active = await window.FourPlayV2.waitForSession(session,
    (id) => fetchJson(`/api/v1/sessions/${encodeURIComponent(id)}`),
    (ms) => new Promise((resolve) => window.setTimeout(resolve, ms)));
  const path = active.connection_grant.webrtc_signaling.offer_path;
  await startBrowserMedia({ dataset: {
    browserMediaUrl: new URL(path, window.location.origin).toString(),
    browserMediaRole: "player", browserMediaPlayer: String(playerNumber),
    browserMediaSeat: state.seatId,
    browserMediaButtons: String(active.runtime_profile?.buttons_per_player ?? ""),
  } });
}

function syncSettingsFromInputs() {
  if (window.FourPlayV2) return;
  state.token = tokenInput.value.trim();
  state.seatId = seatInput.value.trim() || "windows-seat-1";
  state.destinationIp = destinationInput.value.trim();
  localStorage.setItem("fourplay.clientToken", state.token);
  localStorage.setItem("fourplay.seatId", state.seatId);
  localStorage.setItem("fourplay.destinationIp", state.destinationIp);
  renderDestinationWarning();
}

function renderDestinationWarning() {
  if (!destinationWarning) return;
  const warning = destinationAddressWarning(state.destinationIp);
  destinationWarning.textContent = warning || "";
  destinationWarning.hidden = !warning;
}

function destinationAddressWarning(destinationAddress) {
  const destination = String(destinationAddress || "").trim();
  if (!destination) {
    return "Enter this Windows seat PC's LAN IP so the game stream is sent here.";
  }
  const controlPlaneHost = window.location.hostname;
  if (destination === controlPlaneHost) {
    return "This matches the 4-Play server IP. Use this Windows seat PC's LAN IP instead, or the stream will be sent back to the server.";
  }
  if (destination === "127.0.0.1" || destination.toLowerCase() === "localhost") {
    return "Loopback will only work if the game is running on this same PC. Use this Windows seat PC's LAN IP for the Linux server.";
  }
  return "";
}

function renderSessionDetails(session) {
  const openSlots = (session.player_slots || []).filter((slot) => slot.state === "open");
  const ownOccupied = (session.player_slots || []).filter(
    (slot) => slot.state === "occupied" && slot.seat_id === state.seatId,
  );
  const ownDisconnected = (session.player_slots || []).filter(
    (slot) => slot.state === "disconnected" && slot.seat_id === state.seatId,
  );
  renderDetails(`
    <div class="item">
      <h3>${escapeHtml(sessionTitle(session))}</h3>
      <p class="muted">Running on ${escapeHtml(session.runtime_host_id || "unknown host")}.</p>
      <div>
        <span class="pill ${sessionStateTone(session.state)}">${escapeHtml(session.state || "active")}</span>
        ${destinationMismatchBadge(session)}
      </div>
      ${session.failure_reason ? `<p class="pill bad">${escapeHtml(session.failure_reason)}</p>` : ""}
      <div class="form-actions">
        ${ownOccupied.map((slot) => `<button type="button" data-leave-player="${escapeHtml(String(slot.player_number))}">Leave ${escapeHtml(slot.label || `P${slot.player_number}`)}</button>`).join("")}
        ${ownDisconnected.map((slot) => `<button type="button" data-rejoin-player="${escapeHtml(String(slot.player_number))}">Rejoin ${escapeHtml(slot.label || `P${slot.player_number}`)}</button>`).join("")}
        ${openSlots.map((slot) => `<button type="button" data-join-player="${escapeHtml(String(slot.player_number))}">Join ${escapeHtml(slot.label || `P${slot.player_number}`)}</button>`).join("")}
        <button type="button" id="spectate-session">Spectate</button>
      </div>
      <p class="muted">Join/rejoin/leave update real player slots. Join/rejoin prepares safe media runtime details for this seat.</p>
      <div id="client-action-result"></div>
    </div>
  `);
  document.querySelectorAll("[data-leave-player]").forEach((button) => {
    button.addEventListener("click", () =>
      leavePlayerSlot(session, Number(button.dataset.leavePlayer)),
    );
  });
  document.querySelectorAll("[data-join-player]").forEach((button) => {
    button.addEventListener("click", () =>
      joinPlayerSlot(session, Number(button.dataset.joinPlayer), false),
    );
  });
  document.querySelectorAll("[data-rejoin-player]").forEach((button) => {
    button.addEventListener("click", () =>
      joinPlayerSlot(session, Number(button.dataset.rejoinPlayer), true),
    );
  });
  document
    .querySelector("#spectate-session")
    .addEventListener("click", () => spectateSession(session));
}

async function joinPlayerSlot(session, playerNumber, rejoin) {
  if (window.FourPlayV2?.launching) return;
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!Number.isInteger(playerNumber) || playerNumber <= 0) {
    result.innerHTML = `<p class="pill bad">Invalid player slot.</p>`;
    return;
  }
  if (window.FourPlayV2) window.FourPlayV2.launching = true;
  result.innerHTML = `<p class="pill warn">${rejoin ? "Rejoining" : "Joining"} player ${escapeHtml(String(playerNumber))}...</p>`;
  try {
    if (!rejoin) {
      await clientApi.reservePlayerSlot(session.id, playerNumber, state.seatId);
    }
    const connected = await clientApi.connectPlayerSlot(session.id, playerNumber, state.seatId);
    rememberRuntime(publicPlayerHandoff(connected, playerNumber));
    if (window.FourPlayV2) { await openV2Player(connected, playerNumber); await refresh(); return; }
    const slot = (connected.player_slots || []).find(
      (candidate) => candidate.player_number === playerNumber,
    );
    result.innerHTML = `
      <div class="item">
        <h3>${rejoin ? "Player slot rejoined" : "Player slot joined"}</h3>
        <span class="pill good">${escapeHtml(slot?.label || `P${playerNumber}`)}</span>
        <span class="pill">${escapeHtml(connected.id)}</span>
        ${runtimePills(connected.connection_grant)}
        <p class="muted">Slot ownership is live. Use the media command below until the packaged client owns process launch and internal input handoff.</p>
        ${playerRuntimePanel(connected, playerNumber)}
      </div>
    `;
    wireCopyButtons(result);
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  } finally {
    if (window.FourPlayV2) window.FourPlayV2.launching = false;
  }
}

async function leavePlayerSlot(session, playerNumber) {
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!Number.isInteger(playerNumber) || playerNumber <= 0) {
    result.innerHTML = `<p class="pill bad">Invalid player slot.</p>`;
    return;
  }
  result.innerHTML = `<p class="pill warn">Leaving player ${escapeHtml(String(playerNumber))}...</p>`;
  try {
    const updated = await clientApi.releasePlayerSlot(session.id, playerNumber, state.seatId);
    clearLastRuntimeFor(session.id, playerNumber);
    const slot = (updated.player_slots || []).find(
      (candidate) => candidate.player_number === playerNumber,
    );
    result.innerHTML = `
      <div class="item">
        <h3>Player slot left</h3>
        <span class="pill warn">${escapeHtml(slot?.label || `P${playerNumber}`)}</span>
        <span class="pill">${escapeHtml(slot?.state || "disconnected")}</span>
        <p class="muted">Your slot is released. If nobody is playing or watching, the game will close automatically.</p>
      </div>
    `;
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

async function spectateSession(session) {
  if (window.FourPlayV2?.launching) return;
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!state.destinationIp) {
    result.innerHTML = `<p class="pill bad">Enter this seat's Media IP before spectating.</p>`;
    return;
  }
  if (window.FourPlayV2) window.FourPlayV2.launching = true;
  result.innerHTML = `<p class="pill warn">Creating spectator feed...</p>`;
  try {
    const grant = await clientApi.spectate(session.id, state.seatId, state.destinationIp);
    rememberRuntime(publicSpectatorHandoff(session, grant));
    if (window.FourPlayV2) {
      if (!grant.webrtc_signaling?.offer_path) throw Error("Browser viewing is not ready for this game. Please ask the site operator.");
      await startBrowserMedia({ dataset: {
        browserMediaUrl: new URL(grant.webrtc_signaling.offer_path, window.location.origin).toString(),
        browserMediaRole: "spectator", browserMediaGrant: grant.id, browserMediaSeat: state.seatId,
      } });
      await refresh(); return;
    }
    result.innerHTML = `
      <div class="item">
        <h3>Spectator feed created</h3>
        <span class="pill good">media ${escapeHtml(String(grant.media_udp_port))}</span>
        <span class="pill">${escapeHtml(grant.id)}</span>
        <p class="muted">Spectating does not reserve a player slot or send input.</p>
        ${spectatorRuntimeCommands(grant)}
      </div>
    `;
    wireCopyButtons(result);
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  } finally {
    if (window.FourPlayV2) window.FourPlayV2.launching = false;
  }
}

function runtimePills(grant) {
  if (!grant) {
    return `<span class="pill warn">runtime grant unavailable</span>`;
  }
  const browserMedia = grant.webrtc_signaling
    ? `<span class="pill good">browser media ready</span>`
    : "";
  return `
    <span class="pill">media ${escapeHtml(String(grant.media_udp_port || "unknown"))}</span>
    <span class="pill">input ${escapeHtml(String(grant.input_udp_port || "unknown"))}</span>
    ${browserMedia}
  `;
}

function publicPlayerHandoff(session, playerNumber) {
  const grant = session.connection_grant || {};
  return {
    mode: "player",
    session_id: session.id,
    game_id: session.game_id,
    display_name: session.display_name || session.game_id,
    media: publicMediaHandoff(grant.media_udp_port, grant.webrtc_signaling),
    input: grant.runtime_host_address && grant.input_udp_port
      ? {
          destination: `${grant.runtime_host_address}:${grant.input_udp_port}`,
          player_number: playerNumber,
          token_required: true,
        }
      : null,
  };
}

function publicSpectatorHandoff(session, grant) {
  return {
    mode: "spectator",
    session_id: session.id,
    game_id: session.game_id,
    display_name: session.display_name || session.game_id,
    media: publicMediaHandoff(grant.media_udp_port, grant.webrtc_signaling),
    input: null,
  };
}

function publicMediaHandoff(mediaPort, webrtcSignaling) {
  if (!mediaPort) return null;
  return {
    udp_port: mediaPort,
    receiver_url: receiverUrl(mediaPort),
    runtime_command_hint: `seat-client-runtime media --port ${mediaPort} --ffplay-path <path-to-ffplay>`,
    webrtc_signaling: webrtcSignaling || null,
  };
}

function rememberRuntime(runtime) {
  state.lastRuntime = runtime;
  if (!window.FourPlayV2) localStorage.setItem("fourplay.lastRuntime", JSON.stringify(runtime));
}

function loadLastRuntime() {
  try {
    return JSON.parse(localStorage.getItem("fourplay.lastRuntime") || "null");
  } catch {
    return null;
  }
}

function clearLastRuntimeFor(sessionId, playerNumber) {
  if (
    state.lastRuntime?.mode === "player" &&
    state.lastRuntime.session_id === sessionId &&
    state.lastRuntime.input?.player_number === playerNumber
  ) {
    state.lastRuntime = null;
    if (!window.FourPlayV2) localStorage.removeItem("fourplay.lastRuntime");
  }
}

function playerRuntimePanel(session, playerNumber) {
  const handoff = publicPlayerHandoff(session, playerNumber);
  if (!handoff.media || !handoff.input) {
    return `<p class="pill bad">Runtime connection grant is missing; refresh or rejoin this slot.</p>`;
  }
  return `
    <div class="runtime-panel player-mode">
      <h4>Player runtime</h4>
      ${browserMediaPanel(handoff.media.webrtc_signaling, {
        role: "player",
        playerNumber,
        buttonCount: session.runtime_profile?.buttons_per_player,
      })}
      ${mediaReceiverCommands(handoff.media.udp_port)}
      <p><strong>Input endpoint:</strong> ${escapeHtml(handoff.input.destination)} for player ${escapeHtml(String(handoff.input.player_number))}</p>
      <p class="muted">Input token is intentionally not displayed or persisted. The packaged client/runtime handoff will consume it internally after start, join, or rejoin.</p>
    </div>
  `;
}

function spectatorRuntimeCommands(grant) {
  if (!grant?.media_udp_port) {
    return `<p class="pill bad">Spectator media grant is missing; try creating the spectator feed again.</p>`;
  }
  return `
    <div class="runtime-panel spectator-mode">
      <h4>Spectator runtime</h4>
      ${browserMediaPanel(grant.webrtc_signaling, {
        role: "spectator",
        spectatorGrantId: grant.id,
      })}
      ${mediaReceiverCommands(grant.media_udp_port)}
      <p class="muted">Spectator mode opens media only; it does not reserve a player slot or send input.</p>
    </div>
  `;
}

function mediaReceiverCommands(mediaPort) {
  if (!mediaPort) {
    return "";
  }
  const url = receiverUrl(mediaPort);
  const ffplay = `ffplay -f mpegts -fflags nobuffer -flags low_delay -framedrop -probesize 32768 -analyzeduration 0 "${url}"`;
  const runtime = `cargo run -p seat-client-runtime -- media --port ${mediaPort} --ffplay-path .\\path\\to\\ffplay.exe`;
  return `
    <div class="receiver-commands">
      <p><strong>Receiver URL:</strong></p>
      <pre class="command">${escapeHtml(url)}</pre>
      <button type="button" data-copy-text="${escapeHtml(url)}">Copy receiver URL</button>
      <p><strong>Seat runtime media command:</strong></p>
      <pre class="command">${escapeHtml(runtime)}</pre>
      <button type="button" data-copy-text="${escapeHtml(runtime)}">Copy runtime media command</button>
      <p><strong>ffplay command:</strong></p>
      <pre class="command">${escapeHtml(ffplay)}</pre>
      <button type="button" data-copy-text="${escapeHtml(ffplay)}">Copy ffplay command</button>
    </div>
  `;
}

function browserMediaPanel(signaling, context) {
  if (!signaling?.offer_path) {
    return "";
  }
  const absoluteOfferUrl = new URL(signaling.offer_path, window.location.origin).toString();
  const playerNumber = context?.playerNumber ? ` data-browser-media-player="${escapeHtml(String(context.playerNumber))}"` : "";
  const spectatorGrantId = context?.spectatorGrantId ? ` data-browser-media-grant="${escapeHtml(context.spectatorGrantId)}"` : "";
  return `
    <div class="browser-media-panel">
      <p><strong>Browser-native media:</strong> WebRTC signaling is available for this handoff.</p>
      <pre class="command">${escapeHtml(absoluteOfferUrl)}</pre>
      <button
        type="button"
        data-browser-media-url="${escapeHtml(absoluteOfferUrl)}"
        data-browser-media-role="${escapeHtml(context?.role || "spectator")}"
        ${playerNumber}
        ${context?.role === "player" ? `data-browser-media-buttons="${escapeHtml(String(context.buttonCount ?? ""))}"` : ""}
        ${spectatorGrantId}
      >Open in browser player</button>
      <button type="button" data-copy-text="${escapeHtml(absoluteOfferUrl)}">Copy WebRTC offer URL</button>
      <p class="muted">This is token-safe signaling metadata. The private input token is not exposed to the page.</p>
    </div>
  `;
}

function receiverUrl(mediaPort) {
  return `udp://0.0.0.0:${mediaPort}?fifo_size=1000000&overrun_nonfatal=1`;
}

function wireCopyButtons(container) {
  container.querySelectorAll("[data-copy-text]").forEach((button) => {
    const originalText = button.textContent;
    button.addEventListener("click", async () => {
      await navigator.clipboard.writeText(button.dataset.copyText || "");
      button.textContent = "Copied";
      window.setTimeout(() => {
        button.textContent = originalText;
      }, 1400);
    });
  });
  wireBrowserMediaButtons(container);
}

function wireBrowserMediaButtons(container) {
  container.querySelectorAll("[data-browser-media-url]").forEach((button) => {
    button.addEventListener("click", () => startBrowserMedia(button));
  });
}

async function startBrowserMedia(button) {
  if (state.browserExitPending || state.browserMediaNegotiating) return;
  if (state.browserJoinPending && !button.dataset.browserMediaPromotion) return;
  syncSettingsFromInputs();
  if (!window.RTCPeerConnection) {
    setBrowserMediaState("This browser does not expose RTCPeerConnection.");
    return;
  }
  const offerUrl = button.dataset.browserMediaUrl;
  if (!offerUrl) {
    setBrowserMediaState("Missing WebRTC signaling URL.");
    return;
  }
  stopBrowserMedia();
  state.browserMediaRetry = { ...button.dataset };
  state.browserMediaNegotiating = true;
  reconnectBrowserMediaButton.disabled = true;
  const sessionMatch = new URL(offerUrl).pathname.match(/^\/api\/v1\/sessions\/([^/]+)\/webrtc\/offer$/);
  state.browserMediaContext = sessionMatch ? {
    sessionId: decodeURIComponent(sessionMatch[1]),
    seatId: button.dataset.browserMediaSeat || state.seatId,
    role: button.dataset.browserMediaRole || "spectator",
    playerNumber: Number(button.dataset.browserMediaPlayer),
    grantId: button.dataset.browserMediaGrant,
  } : null;
  browserPlayer.hidden = false;
  document.body?.classList.add("browser-playing");
  browserPlayer.scrollIntoView({ behavior: "smooth", block: "start" });
  setBrowserMediaState("Creating browser media peer...");

  const peer = new RTCPeerConnection({ iceServers: [] });
  let stream = new MediaStream();
  state.browserMediaPeer = peer;
  state.browserMediaStream = stream;
  browserMediaVideo.playsInline = true;
  if (state.browserMediaContext?.role === "spectator") startSpectatorOverlay();
  browserReceiverStats.textContent = "Waiting for receiver statistics...";
  state.browserMediaMonitor = new FourPlayBrowserMediaMonitor(peer, (stats) => {
    if (state.browserMediaPeer !== peer) return;
    browserReceiverStats.textContent = stats ?
      `${stats.stalled ? "Video stalled — try Play / resume, then reconnect media if needed. " : ""}${stats.fps.toFixed(1)} fps · ${stats.decodeCounter ? stats.frames : "unknown"} decoded · ${stats.dropped} dropped · ${stats.lost} lost packets · jitter ${stats.jitterMs.toFixed(1)} ms · RTT ${stats.rttMs === null ? "unknown" : stats.rttMs.toFixed(1) + " ms"} · surface ${browserMediaVideo.videoWidth || 0}×${browserMediaVideo.videoHeight || 0} · ${browserMediaVideo.paused ? "paused" : "playing"}` :
      "Receiver statistics unavailable in this browser.";
  });
  state.browserMediaMonitor.start();

  peer.addTransceiver("video", { direction: "recvonly" });
  peer.addTransceiver("audio", { direction: "recvonly" });
  const inputStatus = document.querySelector("#browser-input-state");
  inputStatus.textContent = "Spectator mode: view only.";
  if (button.dataset.browserMediaRole === "player") {
    const channel = peer.createDataChannel("4play-input", { ordered: false, maxRetransmits: 0 });
    const input = new FourPlayBrowserInput(channel, browserPlayer);
    const session = state.sessions.find((session) => session.id === state.browserMediaContext?.sessionId);
    input.bindTouch(document.querySelector("#touch-controls"),
      button.dataset.browserMediaButtons ?? session?.runtime_profile?.buttons_per_player);
    state.browserInput = input;
    inputStatus.textContent = "Connecting game controls...";
    channel.addEventListener("open", () => {
      if (state.browserMediaPeer !== peer) return;
      if (peer.connectionState === "connected") input.start();
      browserMediaVideo.focus();
      inputStatus.textContent = input.touch?.running
        ? "Touch controls ready. Hold the left pad to move; combine action buttons on the right. Coin adds credit; Start begins play."
        : "Controls ready. Click the video to play: W/A/S/D move; J/K/L and M/,/. attack; 1 coin; 2 start; Esc to leave (fullscreen: Esc twice).";
    });
    channel.addEventListener("close", () => {
      input.stop();
      if (state.browserMediaPeer === peer) {
        stopBrowserInput();
        inputStatus.textContent = "Game controls disconnected. Reopen the browser player to reconnect.";
      }
    });
  }
  peer.addEventListener("track", (event) => {
    if (state.browserMediaPeer !== peer) return;
    stream.addTrack(event.track);
    // Attach a populated composite, including when video follows audio. Some
    // WebKit versions do not refresh an initially empty/audio-only attachment.
    stream = new MediaStream(stream.getTracks());
    state.browserMediaStream = stream;
    browserMediaVideo.srcObject = stream;
    setBrowserMediaState(`Attached remote ${event.track.kind} track.`);
    browserMediaVideo.play().catch((error) => {
      if (state.browserMediaPeer !== peer || error.name === "AbortError") return;
      setBrowserMediaState(`Browser media attached; press play if autoplay was blocked. ${error.message}`);
    });
  });
  peer.addEventListener("connectionstatechange", () => {
    if (state.browserMediaPeer !== peer) return;
    if (["failed", "disconnected", "closed"].includes(peer.connectionState)) {
      state.browserInput?.stop();
      inputStatus.textContent = "Controls released while media is disconnected. Reconnect media to resume.";
    } else if (peer.connectionState === "connected") {
      state.browserInput?.start();
    }
    setBrowserMediaState(`Browser media connection: ${peer.connectionState}`);
  });
  peer.addEventListener("iceconnectionstatechange", () => {
    if (state.browserMediaPeer !== peer) return;
    if (["failed", "disconnected"].includes(peer.iceConnectionState)) {
      state.browserInput?.stop();
      setBrowserMediaState(`Browser media ICE state: ${peer.iceConnectionState}`);
    } else if (["connected", "completed"].includes(peer.iceConnectionState)
        && peer.connectionState === "connected") {
      state.browserInput?.start();
    }
  });

  try {
    const offer = await peer.createOffer();
    if (state.browserMediaPeer !== peer) return;
    await peer.setLocalDescription(offer);
    await waitForIceGathering(peer, 2000);
    if (state.browserMediaPeer !== peer) return;
    const body = {
      seat_id: state.browserMediaContext?.seatId || state.seatId,
      role: button.dataset.browserMediaRole || "spectator",
      sdp_offer: peer.localDescription?.sdp || offer.sdp,
    };
    if (button.dataset.browserMediaPlayer) {
      body.player_number = Number(button.dataset.browserMediaPlayer);
    }
    if (button.dataset.browserMediaGrant) {
      body.spectator_grant_id = button.dataset.browserMediaGrant;
    }
    setBrowserMediaState("Sending WebRTC offer to session publisher...");
    const answer = await postJson(offerUrl, body);
    if (state.browserMediaPeer !== peer) return;
    await peer.setRemoteDescription({ type: "answer", sdp: answer.sdp_answer });
    if (state.browserMediaPeer !== peer) return;
    if (answer.input_peer_id && body.player_number) {
      const leaseUrl = `${offerUrl.replace(/\/webrtc\/offer$/, "")}/player-slots/${body.player_number}/webrtc-input/lease`;
      state.browserInputLeaseTimer = window.setInterval(async () => {
        if (state.browserMediaPeer !== peer) return;
        try {
          await postJson(leaseUrl, { seat_id: body.seat_id, peer_id: answer.input_peer_id });
        } catch {
          if (state.browserMediaPeer !== peer) return;
          stopBrowserInput();
          inputStatus.textContent = "Game control authorization ended. Reconnect to your player slot.";
        }
      }, 20000);
    }
    setBrowserMediaState("Applied WebRTC answer; waiting for media packets...");
  } catch (error) {
    if (state.browserMediaPeer !== peer) return;
    setBrowserMediaState(`Browser media failed: ${error.message || error}`);
    peer.close();
    if (state.browserMediaPeer === peer) {
      stopBrowserInput();
      state.browserMediaMonitor?.stop();
      state.browserMediaMonitor = null;
      stream.getTracks().forEach((track) => track.stop());
      browserMediaVideo.srcObject = null;
      state.browserMediaPeer = null;
      state.browserMediaStream = null;
    }
  } finally {
    state.browserMediaNegotiating = false;
    reconnectBrowserMediaButton.disabled = !state.browserMediaRetry || state.browserExitPending;
  }
}

function stopBrowserMedia() {
  document.body?.classList.remove("browser-playing");
  stopSpectatorOverlay();
  state.browserMediaMonitor?.stop();
  state.browserMediaMonitor = null;
  state.browserMediaRetry = null;
  reconnectBrowserMediaButton.disabled = true;
  stopBrowserInput();
  if (state.browserMediaPeer) {
    state.browserMediaPeer.close();
    state.browserMediaPeer = null;
  }
  if (state.browserMediaStream) {
    state.browserMediaStream.getTracks().forEach((track) => track.stop());
    state.browserMediaStream = null;
  }
  if (browserMediaVideo) {
    browserMediaVideo.pause();
    browserMediaVideo.srcObject = null;
  }
  setBrowserMediaState("Browser-native media stopped.");
  state.browserMediaContext = null;
}

async function resumeBrowserMedia() {
  const peer = state.browserMediaPeer;
  if (!peer || !browserMediaVideo.srcObject) return;
  try {
    await browserMediaVideo.play();
    if (state.browserMediaPeer === peer) browserMediaVideo.focus();
  } catch (error) {
    if (state.browserMediaPeer === peer) setBrowserMediaState(`Playback could not start: ${error.message}`);
  }
}

function stopSpectatorOverlay() {
  if (state.spectatorSlotsTimer !== null) window.clearInterval(state.spectatorSlotsTimer);
  state.spectatorSlotsTimer = null;
  state.spectatorOverlay?.stop();
  state.spectatorOverlay = null;
  document.querySelector("#spectator-actions").hidden = true;
}

function startSpectatorOverlay() {
  const context = state.browserMediaContext;
  const panel = document.querySelector("#spectator-actions");
  panel.hidden = false;
  renderSpectatorSlots(state.sessions.find((session) => session.id === context.sessionId));
  state.spectatorOverlay = new FourPlaySpectator.SpectatorOverlay(browserPlayer, panel, {
    onShow: () => refreshSpectatorSlots(context),
  });
  state.spectatorOverlay.start();
  browserMediaVideo.focus();
  state.spectatorSlotsTimer = window.setInterval(() => refreshSpectatorSlots(context), 5000);
}

async function refreshSpectatorSlots(context) {
  if (state.browserMediaContext !== context || state.browserJoinPending || state.spectatorSlotsLoading) return;
  state.spectatorSlotsLoading = true;
  try {
    const result = await clientApi.loadActiveSessions();
    if (state.browserMediaContext !== context || state.browserJoinPending) return;
    renderSpectatorSlots((result.sessions || []).find((session) => session.id === context.sessionId));
  } catch {
    if (state.browserMediaContext !== context || state.browserJoinPending) return;
    document.querySelector("#spectator-player-slots").replaceChildren();
    document.querySelector("#spectator-action-status").textContent = "Cannot refresh player slots. Media can continue; try again shortly.";
  } finally {
    state.spectatorSlotsLoading = false;
  }
}

function renderSpectatorSlots(session) {
  const context = state.browserMediaContext;
  if (context?.role !== "spectator") return;
  const container = document.querySelector("#spectator-player-slots");
  const focusedPlayer = container.contains(document.activeElement) ? document.activeElement.dataset.playerNumber : null;
  const available = ["active", "ready"].includes(session?.state) ? FourPlaySpectator.joinableSlots(session, context.seatId) : [];
  const buttons = available.map((slot) => {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.playerNumber = String(slot.player_number);
    button.textContent = `Join ${slot.presentation?.label || slot.label || `P${slot.player_number}`}`;
    button.disabled = state.browserJoinPending;
    button.addEventListener("click", () => joinBrowserSpectator(slot.player_number));
    return button;
  });
  container.replaceChildren(...buttons);
  if (focusedPlayer) {
    const next = buttons.find((button) => button.dataset.playerNumber === focusedPlayer);
    if (next) next.focus(); else browserMediaVideo.focus();
  }
  document.querySelector("#spectator-action-status").textContent = !session ? "Session unavailable. You can leave this view." :
    available.length ? "Choose a player slot to join. Joystick/arrows select; A/Enter confirms." : "No open player slots. Keep watching — positions refresh automatically.";
}

async function joinBrowserSpectator(playerNumber) {
  const context = state.browserMediaContext;
  if (context?.role !== "spectator" || state.browserJoinPending || state.browserMediaNegotiating) return;
  state.browserJoinPending = true;
  state.spectatorOverlay?.setBusy(true);
  document.querySelector("#spectator-player-slots").querySelectorAll("button").forEach((button) => { button.disabled = true; });
  document.querySelector("#spectator-action-status").textContent = `Joining player ${playerNumber}...`;
  let acquired = false;
  let handedOff = false;
  try {
    const session = await FourPlaySpectator.acquirePlayer(clientApi, context, playerNumber,
      () => state.browserMediaContext === context && !state.browserExitPending);
    if (!session) return;
    acquired = true;
    const signaling = session.connection_grant?.webrtc_signaling?.offer_path;
    if (!signaling) throw Error("This player slot has no browser media handoff.");
    const offerUrl = new URL(signaling, window.location.origin).toString();
    rememberRuntime(publicPlayerHandoff(session, playerNumber));
    handedOff = true;
    await startBrowserMedia({ dataset: {
      browserMediaUrl: offerUrl,
      browserMediaRole: "player", browserMediaPlayer: String(playerNumber),
      browserMediaSeat: context.seatId, browserMediaPromotion: "true",
      browserMediaButtons: String(session.runtime_profile?.buttons_per_player ?? ""),
      ...(context.grantId ? { browserMediaGrant: context.grantId } : {}),
    } });
    if (state.browserMediaRetry?.browserMediaPromotion) delete state.browserMediaRetry.browserMediaPromotion;
    if (context.grantId) {
      try {
        await clientApi.releaseSpectatorGrant(context.sessionId, context.grantId, context.seatId);
        if (state.browserMediaContext?.role === "player" && state.browserMediaContext.sessionId === context.sessionId
            && state.browserMediaContext.grantId === context.grantId) {
          state.browserMediaContext.grantId = undefined;
          if (state.browserMediaRetry) delete state.browserMediaRetry.browserMediaGrant;
        }
      } catch {
        if (state.browserMediaContext?.sessionId === context.sessionId) {
          setBrowserMediaState("Player handoff prepared, but the old spectator grant could not be released; it will expire automatically.");
        }
      }
    }
    await refresh();
  } catch (error) {
    if (acquired && !handedOff) {
      try { await clientApi.disconnectPlayerSlot(context.sessionId, playerNumber, context.seatId); }
      catch { error = Error(`${error.message} Player-slot cleanup also failed; use the session Leave action.`); }
    }
    if (state.browserMediaContext === context) {
      document.querySelector("#spectator-action-status").textContent = `Could not join: ${error.message}. You are still spectating.`;
    }
  } finally {
    state.browserJoinPending = false;
    state.spectatorOverlay?.setBusy(false);
    if (state.browserMediaContext === context) {
      document.querySelector("#spectator-player-slots").querySelectorAll("button").forEach((button) => { button.disabled = false; });
    }
  }
}

async function leaveBrowserGame() {
  if (state.browserExitPending || !state.browserMediaContext) return;
  state.browserExitPending = true;
  const context = state.browserMediaContext;
  stopBrowserMedia();
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    if (context?.role === "player" && context.playerNumber > 0) {
      await clientApi.releasePlayerSlot(context.sessionId, context.playerNumber, context.seatId);
      if (context.grantId) await clientApi.releaseSpectatorGrant(context.sessionId, context.grantId, context.seatId);
      clearLastRuntimeFor(context.sessionId, context.playerNumber);
    } else if (context?.grantId) {
      await clientApi.releaseSpectatorGrant(context.sessionId, context.grantId, context.seatId);
      if (state.lastRuntime?.session_id === context.sessionId && state.lastRuntime?.mode === "spectator") {
        state.lastRuntime = null;
        if (!window.FourPlayV2) localStorage.removeItem("fourplay.lastRuntime");
      }
    }
    browserPlayer.hidden = true;
    document.querySelector("#browse-games").focus();
    document.querySelector("#games").scrollIntoView({ behavior: "smooth", block: "start" });
    await refresh();
  } catch (error) {
    state.browserMediaContext = context;
    browserPlayer.hidden = false;
    setBrowserMediaState(`Media stopped, but leaving the seat failed: ${error.message}. Use the session's Leave action to retry.`);
  } finally {
    state.browserExitPending = false;
  }
}

function stopBrowserInput() {
  if (state.browserInputLeaseTimer !== null) window.clearInterval(state.browserInputLeaseTimer);
  state.browserInputLeaseTimer = null;
  state.browserInput?.stop();
  state.browserInput = null;
}

function setBrowserMediaState(message) {
  if (!browserMediaState) return;
  browserMediaState.textContent = message;
}

function waitForIceGathering(peer, timeoutMs) {
  if (peer.iceGatheringState === "complete") {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    const timeout = window.setTimeout(done, timeoutMs);
    peer.addEventListener("icegatheringstatechange", onStateChange);

    function onStateChange() {
      if (peer.iceGatheringState === "complete") {
        done();
      }
    }

    function done() {
      window.clearTimeout(timeout);
      peer.removeEventListener("icegatheringstatechange", onStateChange);
      resolve();
    }
  });
}

function renderLastRuntimeHint() {
  const runtime = state.lastRuntime;
  const container = document.querySelector("#runtime-status");
  if (!container || !runtime) {
    if (container) container.innerHTML = "";
    return;
  }
  const label =
    runtime.mode === "player"
      ? `Last player runtime: ${runtime.display_name || runtime.game_id} P${runtime.input?.player_number || "?"}`
      : `Last spectator runtime: ${runtime.display_name || runtime.game_id}`;
  container.innerHTML = `
    <div class="runtime-status">
      <span class="pill warn">${escapeHtml(label)}</span>
      <button type="button" id="clear-runtime-status">Clear</button>
    </div>
  `;
  document.querySelector("#clear-runtime-status").addEventListener("click", () => {
    state.lastRuntime = null;
    localStorage.removeItem("fourplay.lastRuntime");
    renderLastRuntimeHint();
  });
}

function renderDetails(html) {
  const container = document.querySelector("#details");
  container.classList.remove("muted");
  container.innerHTML = html;
}

function replaceChildren(container, children) {
  container.classList.remove("muted");
  container.replaceChildren(...children);
  if (!children.length) {
    container.classList.add("muted");
    container.appendChild(document.querySelector("#empty-template").content.cloneNode(true));
  }
}

function item() {
  return document.createElement("article");
}

function setText(selector, text) {
  document.querySelector(selector).textContent = text;
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

if (window.FourPlayV2) {
  document.querySelector(".topbar h1").textContent = "4-Play Arcade";
  window.FourPlayV2.bindRail(document.querySelector("#games"), (id) => {
    librarySelection.select(id); updateFeaturedGame();
  });
  window.FourPlayV2.bootstrap().then((seat) => {
    state.seatId = seat.seat_id; state.destinationIp = seat.destination_address;
    seatInput.value = state.seatId; destinationInput.value = state.destinationIp;
    destinationWarning.hidden = true;
    refresh();
    window.setInterval(() => {
      if (!document.hidden && !state.browserMediaPeer && !window.FourPlayV2.launching && !state.browserJoinPending) refresh();
    }, 15000);
  }).catch((error) => {
    setText("#primary-title", "This arcade is not ready yet");
    setText("#primary-detail", error.message);
  });
} else refresh();
