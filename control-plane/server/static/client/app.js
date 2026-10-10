const tokenInput = document.querySelector("#api-token");
const seatInput = document.querySelector("#seat-id");
const destinationInput = document.querySelector("#destination-ip");
const saveSettingsButton = document.querySelector("#save-settings");
const refreshButton = document.querySelector("#refresh");
const gameFilter = document.querySelector("#game-filter");

const state = {
  token: localStorage.getItem("fourplay.clientToken") || "",
  seatId: localStorage.getItem("fourplay.seatId") || "windows-seat-1",
  destinationIp: localStorage.getItem("fourplay.destinationIp") || "",
  lastRuntime: loadLastRuntime(),
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
  async spectate(sessionId, seatId, destinationAddress) {
    return postJson(`/api/v1/sessions/${sessionId}/spectators`, {
      seat_id: seatId,
      destination_address: destinationAddress,
    });
  },
};

tokenInput.value = state.token;
seatInput.value = state.seatId;
destinationInput.value = state.destinationIp;

saveSettingsButton.addEventListener("click", () => {
  state.token = tokenInput.value.trim();
  state.seatId = seatInput.value.trim() || "windows-seat-1";
  state.destinationIp = destinationInput.value.trim();
  localStorage.setItem("fourplay.clientToken", state.token);
  localStorage.setItem("fourplay.seatId", state.seatId);
  localStorage.setItem("fourplay.destinationIp", state.destinationIp);
  refresh();
});

refreshButton.addEventListener("click", refresh);
gameFilter.addEventListener("input", renderGames);
document.querySelector("#browse-games").addEventListener("click", () => {
  document.querySelector("#games").scrollIntoView({ behavior: "smooth", block: "start" });
});
document.querySelector("#browse-sessions").addEventListener("click", () => {
  document.querySelector("#sessions").scrollIntoView({ behavior: "smooth", block: "start" });
});

function authHeaders() {
  if (!state.token) return {};
  return { Authorization: `Bearer ${state.token}` };
}

async function fetchJson(path) {
  const response = await fetch(path, { headers: authHeaders() });
  if (!response.ok) {
    const body = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${body}`);
  }
  return response.json();
}

async function postJson(path, body) {
  const response = await fetch(path, {
    method: "POST",
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
    const [games, sessions] = await Promise.all([
      clientApi.loadCatalog(),
      clientApi.loadActiveSessions(),
    ]);
    state.games = games.games || [];
    state.sessions = sessions.sessions || [];
    renderHome();
    renderGames();
    renderSessions();
  } catch (error) {
    setText("#primary-title", "Cannot reach 4-Play");
    setText("#primary-detail", String(error));
    renderDetails(`
      <div class="item">
        <h3>Connection problem</h3>
        <p class="muted">Check the seat token, network, and control-plane service.</p>
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
    setText("#primary-detail", `Seat ${state.seatId} has a disconnected player slot waiting.`);
  } else if (openSession) {
    setText("#primary-title", `Join ${openSession.display_name || openSession.game_id}`);
    setText("#primary-detail", "An active game has an open player slot.");
  } else if (state.sessions.length) {
    setText("#primary-title", "Spectate active games");
    setText("#primary-detail", "Games are running, but no open player slot is currently advertised.");
  } else {
    setText("#primary-title", "Choose a game");
    setText("#primary-detail", `${state.games.length} games are available.`);
  }
  renderLastRuntimeHint();
}

function findOwnDisconnectedSlot() {
  for (const session of state.sessions) {
    const slot = (session.player_slots || []).find(
      (candidate) => candidate.seat_id === state.seatId && candidate.state === "disconnected",
    );
    if (slot) {
      return { session, slot, displayName: session.display_name || session.game_id };
    }
  }
  return null;
}

function renderGames() {
  const query = gameFilter.value.trim().toLowerCase();
  const games = state.games.filter((game) => {
    const metadata = game.metadata || {};
    const haystack = [
      game.display_name,
      game.id,
      metadata.genre,
      metadata.manufacturer,
      metadata.release_year,
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return haystack.includes(query);
  });
  replaceChildren(document.querySelector("#games"), games.map(renderGame));
}

function renderGame(game) {
  const metadata = game.metadata || {};
  const element = item();
  element.classList.add("clickable");
  element.innerHTML = `
    <div class="thumbnail">${escapeHtml(metadata.genre || "4-Play")}</div>
    <h3>${escapeHtml(game.display_name || game.id)}</h3>
    <span class="pill">${escapeHtml(game.id)}</span>
    <span class="pill">${escapeHtml(metadata.genre || "unknown genre")}</span>
    <span class="pill">${escapeHtml(String(metadata.player_count || game.availability?.[0]?.profile?.max_players || "?"))} players</span>
    <p class="muted">${escapeHtml(metadata.description || "No description yet.")}</p>
  `;
  element.addEventListener("click", () => renderGameDetails(game));
  return element;
}

function renderSessions() {
  replaceChildren(document.querySelector("#sessions"), state.sessions.map(renderSession));
}

function renderSession(session) {
  const element = item();
  const slots = (session.player_slots || [])
    .map((slot) => {
      const tone = slot.state === "open" ? "good" : slot.state === "disconnected" ? "warn" : "";
      const label = slot.label || `P${slot.player_number}`;
      const owner = slot.seat_id ? ` by ${slot.seat_id}` : "";
      return `<span class="pill ${tone}">${escapeHtml(label)}: ${escapeHtml(slot.state || "open")}${escapeHtml(owner)}</span>`;
    })
    .join("");
  element.classList.add("clickable");
  element.innerHTML = `
    <h3>${escapeHtml(session.display_name || session.game_id)}</h3>
    <span class="pill good">${escapeHtml(session.state || "active")}</span>
    <span class="pill">${escapeHtml(String(session.active_spectator_count || 0))} spectators</span>
    <div>${slots}</div>
  `;
  element.addEventListener("click", () => renderSessionDetails(session));
  return element;
}

function renderGameDetails(game) {
  const running = state.sessions.filter((session) => session.game_id === game.id);
  const metadata = game.metadata || {};
  renderDetails(`
    <div class="item">
      <h3>${escapeHtml(game.display_name || game.id)}</h3>
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
      <div id="client-action-result"></div>
    </div>
  `);
  document.querySelector("#start-game").addEventListener("click", () => startGame(game));
}

async function startGame(game) {
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!state.destinationIp) {
    result.innerHTML = `<p class="pill bad">Enter this seat's Media IP before starting a game.</p>`;
    return;
  }
  result.innerHTML = `<p class="pill warn">Requesting session...</p>`;
  try {
    const session = await clientApi.startGame(game.id, state.seatId, state.destinationIp);
    rememberRuntime(publicPlayerHandoff(session, 1));
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
  }
}

function syncSettingsFromInputs() {
  state.token = tokenInput.value.trim();
  state.seatId = seatInput.value.trim() || "windows-seat-1";
  state.destinationIp = destinationInput.value.trim();
  localStorage.setItem("fourplay.clientToken", state.token);
  localStorage.setItem("fourplay.seatId", state.seatId);
  localStorage.setItem("fourplay.destinationIp", state.destinationIp);
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
      <h3>${escapeHtml(session.display_name || session.game_id)}</h3>
      <p class="muted">Running on ${escapeHtml(session.runtime_host_id || "unknown host")}.</p>
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
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!Number.isInteger(playerNumber) || playerNumber <= 0) {
    result.innerHTML = `<p class="pill bad">Invalid player slot.</p>`;
    return;
  }
  result.innerHTML = `<p class="pill warn">${rejoin ? "Rejoining" : "Joining"} player ${escapeHtml(String(playerNumber))}...</p>`;
  try {
    if (!rejoin) {
      await clientApi.reservePlayerSlot(session.id, playerNumber, state.seatId);
    }
    const connected = await clientApi.connectPlayerSlot(session.id, playerNumber, state.seatId);
    rememberRuntime(publicPlayerHandoff(connected, playerNumber));
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
    const updated = await clientApi.disconnectPlayerSlot(session.id, playerNumber, state.seatId);
    clearLastRuntimeFor(session.id, playerNumber);
    const slot = (updated.player_slots || []).find(
      (candidate) => candidate.player_number === playerNumber,
    );
    result.innerHTML = `
      <div class="item">
        <h3>Player slot left</h3>
        <span class="pill warn">${escapeHtml(slot?.label || `P${playerNumber}`)}</span>
        <span class="pill">${escapeHtml(slot?.state || "disconnected")}</span>
        <p class="muted">The slot is disconnected for this seat and can be rejoined from the same client.</p>
      </div>
    `;
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

async function spectateSession(session) {
  const result = document.querySelector("#client-action-result");
  syncSettingsFromInputs();
  if (!state.destinationIp) {
    result.innerHTML = `<p class="pill bad">Enter this seat's Media IP before spectating.</p>`;
    return;
  }
  result.innerHTML = `<p class="pill warn">Creating spectator feed...</p>`;
  try {
    const grant = await clientApi.spectate(session.id, state.seatId, state.destinationIp);
    rememberRuntime(publicSpectatorHandoff(session, grant));
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
  }
}

function runtimePills(grant) {
  if (!grant) {
    return `<span class="pill warn">runtime grant unavailable</span>`;
  }
  return `
    <span class="pill">media ${escapeHtml(String(grant.media_udp_port || "unknown"))}</span>
    <span class="pill">input ${escapeHtml(String(grant.input_udp_port || "unknown"))}</span>
  `;
}

function publicPlayerHandoff(session, playerNumber) {
  const grant = session.connection_grant || {};
  return {
    mode: "player",
    session_id: session.id,
    game_id: session.game_id,
    display_name: session.display_name || session.game_id,
    media: publicMediaHandoff(grant.media_udp_port),
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
    media: publicMediaHandoff(grant.media_udp_port),
    input: null,
  };
}

function publicMediaHandoff(mediaPort) {
  if (!mediaPort) return null;
  return {
    udp_port: mediaPort,
    receiver_url: receiverUrl(mediaPort),
    runtime_command_hint: `seat-client-runtime media --port ${mediaPort} --ffplay-path <path-to-ffplay>`,
  };
}

function rememberRuntime(runtime) {
  state.lastRuntime = runtime;
  localStorage.setItem("fourplay.lastRuntime", JSON.stringify(runtime));
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
    localStorage.removeItem("fourplay.lastRuntime");
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

refresh();
