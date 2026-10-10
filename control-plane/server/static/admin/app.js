const tokenInput = document.querySelector("#api-token");
const producerDestinationInput = document.querySelector("#producer-destination");
const saveTokenButton = document.querySelector("#save-token");
const refreshButton = document.querySelector("#refresh");
const gameFilter = document.querySelector("#game-filter");

const state = {
  token: localStorage.getItem("fourplay.adminToken") || "",
  games: [],
  hosts: [],
  sessions: [],
  activeSessions: [],
  selectedSession: null,
  productionGrants: [],
  producerDestination: localStorage.getItem("fourplay.producerDestination") || "",
};

tokenInput.value = state.token;
producerDestinationInput.value = state.producerDestination;

saveTokenButton.addEventListener("click", () => {
  state.token = tokenInput.value.trim();
  state.producerDestination = producerDestinationInput.value.trim();
  localStorage.setItem("fourplay.adminToken", state.token);
  localStorage.setItem("fourplay.producerDestination", state.producerDestination);
  refresh();
});

refreshButton.addEventListener("click", refresh);
gameFilter.addEventListener("input", renderGames);

function authHeaders() {
  if (!state.token) return {};
  return { Authorization: `Bearer ${state.token}` };
}

async function fetchJson(path, protectedApi = true) {
  const response = await fetch(path, {
    headers: protectedApi ? authHeaders() : {},
  });
  if (!response.ok) {
    const body = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${body}`);
  }
  return response.json();
}

async function refresh() {
  setText("#health-status", "Loading…");
  try {
    const [health, hosts, games, sessions, activeSessions] = await Promise.all([
      fetchJson("/health", false),
      fetchJson("/api/v1/runtime-hosts"),
      fetchJson("/api/v1/games"),
      fetchJson("/api/v1/sessions"),
      fetchJson("/api/v1/active-sessions"),
    ]);

    state.hosts = hosts.hosts || [];
    state.games = games.games || [];
    state.sessions = sessions.sessions || [];
    state.activeSessions = activeSessions.sessions || [];

    setText("#health-status", health.status || "unknown");
    setText("#host-count", String(state.hosts.length));
    setText("#game-count", String(state.games.length));
    setText("#active-session-count", String(state.activeSessions.length));

    renderHosts();
    renderActiveSessions();
    renderGames();
    renderDiagnostics();
    renderSnapshot({ health, hosts, activeSessions, sessions });
  } catch (error) {
    setText("#health-status", "Error");
    renderSnapshot({ error: String(error) });
  }
}

function renderHosts() {
  const container = document.querySelector("#hosts");
  replaceChildren(container, state.hosts.map(renderHost));
}

function renderHost(host) {
  const element = item();
  const statusClass = host.status === "online" ? "good" : "bad";
  element.innerHTML = `
    <h3>${escapeHtml(host.display_name || host.id)}</h3>
    <span class="pill ${statusClass}">${escapeHtml(host.status || "unknown")}</span>
    <span class="pill">${escapeHtml(host.capabilities?.operating_system || "unknown")}</span>
    <span class="pill">${escapeHtml(String(host.active_session_count ?? 0))} active</span>
    <p class="muted">${escapeHtml(host.capabilities?.data_plane_address || "")}</p>
  `;
  return element;
}

function renderActiveSessions() {
  const container = document.querySelector("#active-sessions");
  replaceChildren(container, state.activeSessions.map(renderActiveSession));
}

function renderActiveSession(session) {
  const element = item();
  const fullSession = state.sessions.find((candidate) => candidate.id === session.id);
  const slots = (session.player_slots || [])
    .map((slot) => {
      const label = slot.label || `P${slot.player_number}`;
      const stateText = slot.seat_id
        ? `${slot.state || "occupied"} by ${slot.seat_id}`
        : slot.state || "open";
      return `<span class="pill">${escapeHtml(label)}: ${escapeHtml(stateText)}</span>`;
    })
    .join("");
  element.innerHTML = `
    <h3>${escapeHtml(session.display_name || session.game_id)}</h3>
    <span class="pill good">${escapeHtml(session.state || "active")}</span>
    <span class="pill">${escapeHtml(session.runtime_host_id || "")}</span>
    <span class="pill">${escapeHtml(String(session.active_spectator_count ?? 0))} spectators</span>
    <span class="pill">media ${escapeHtml(String(fullSession?.connection_grant?.media_udp_port || "unknown"))}</span>
    <div>${slots}</div>
    <button type="button" data-session-id="${escapeHtml(session.id)}">Production notes</button>
  `;
  element.querySelector("button").addEventListener("click", () => renderProduction(session));
  return element;
}

function renderGames() {
  const query = gameFilter.value.trim().toLowerCase();
  const filtered = state.games.filter((game) => {
    const haystack = `${game.display_name} ${game.id} ${game.metadata?.genre || ""}`.toLowerCase();
    return haystack.includes(query);
  });
  const container = document.querySelector("#games");
  replaceChildren(container, filtered.map(renderGame));
}

function renderDiagnostics() {
  const container = document.querySelector("#diagnostics");
  const now = Date.now();
  const sessions = state.sessions || [];
  const nonterminal = sessions.filter((session) => !terminalSessionStates().has(session.state));
  const failed = sessions.filter((session) => failureSessionStates().has(session.state));
  const stopping = sessions.filter((session) => session.state === "stopping");
  const stale = nonterminal.filter((session) => {
    const updated = Number(session.updated_unix_ms || 0);
    return updated > 0 && now - updated > 10 * 60 * 1000;
  });
  const cards = [
    diagnosticMetric("Non-terminal", nonterminal.length, "Sessions still in progress or cleanup."),
    diagnosticMetric("Stopping", stopping.length, "Sessions waiting for runtime shutdown."),
    diagnosticMetric("Failed", failed.length, "Terminal sessions that need operator review."),
    diagnosticMetric("Stale >10m", stale.length, "Non-terminal sessions without recent updates."),
  ];
  const details = [...stale, ...failed].slice(0, 8).map(renderDiagnosticSession);
  container.classList.remove("muted");
  container.innerHTML = `
    <div class="diagnostic-grid">${cards.join("")}</div>
    <div class="stack">
      ${
        details.length
          ? details.join("")
          : `<p class="muted">No stale or failed sessions found.</p>`
      }
    </div>
  `;
}

function terminalSessionStates() {
  return new Set(["stopped", "allocation_failed", "launch_failed", "runtime_lost", "terminated"]);
}

function failureSessionStates() {
  return new Set(["allocation_failed", "launch_failed", "runtime_lost", "unhealthy", "terminated"]);
}

function diagnosticMetric(label, value, description) {
  const tone = value > 0 ? "warn" : "good";
  return `
    <article class="diagnostic-card">
      <span class="label">${escapeHtml(label)}</span>
      <strong class="${tone}">${escapeHtml(String(value))}</strong>
      <p class="muted">${escapeHtml(description)}</p>
    </article>
  `;
}

function renderDiagnosticSession(session) {
  const updated = Number(session.updated_unix_ms || 0);
  const age = updated ? `${Math.round((Date.now() - updated) / 1000)}s since update` : "unknown age";
  return `
    <div class="item">
      <h3>${escapeHtml(session.game_id || session.id)}</h3>
      <span class="pill warn">${escapeHtml(session.state || "unknown")}</span>
      <span class="pill">${escapeHtml(session.runtime_host_id || "unknown host")}</span>
      <span class="pill">${escapeHtml(age)}</span>
      <p class="muted">${escapeHtml(session.failure_reason || "No failure reason recorded.")}</p>
    </div>
  `;
}

function renderGame(game) {
  const element = item();
  const metadata = game.metadata || {};
  element.classList.add("clickable");
  element.innerHTML = `
    <h3>${escapeHtml(game.display_name || game.id)}</h3>
    <span class="pill">${escapeHtml(game.id)}</span>
    <span class="pill">${escapeHtml(metadata.genre || "unknown genre")}</span>
    <span class="pill">${escapeHtml(String(metadata.player_count || game.availability?.[0]?.profile?.max_players || "?"))} players</span>
    <p class="muted">${escapeHtml(metadata.description || "No description yet.")}</p>
    <button type="button">Edit metadata</button>
  `;
  element.querySelector("button").addEventListener("click", () => renderGameEditor(game));
  return element;
}

function renderGameEditor(game) {
  const metadata = game.metadata || {};
  const slotCount = metadata.player_count || game.availability?.[0]?.profile?.max_players || 1;
  const editor = document.querySelector("#game-editor");
  editor.classList.remove("muted");
  editor.innerHTML = `
    <form id="metadata-form" class="metadata-form">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Metadata editor</p>
          <h3>${escapeHtml(game.display_name || game.id)}</h3>
        </div>
        <span class="pill">${escapeHtml(game.id)}</span>
      </div>
      ${metadataInput("Sort title", "sort_title", metadata.sort_title)}
      ${metadataInput("Genre", "genre", metadata.genre)}
      ${metadataInput("Manufacturer", "manufacturer", metadata.manufacturer)}
      ${metadataInput("Release year", "release_year", metadata.release_year, "number", "e.g. 1989")}
      ${metadataInput("Player count", "player_count", metadata.player_count, "number", "1-16")}
      ${metadataTextarea("Description", "description", metadata.description)}
      ${metadataTextarea("Control notes", "control_notes", metadata.control_notes)}
      <div class="metadata-grid">
        ${metadataInput("Artwork path", "artwork_path", metadata.artwork_path, "text", "media/game/artwork.png")}
        ${metadataInput("Marquee path", "marquee_path", metadata.marquee_path, "text", "media/game/marquee.png")}
        ${metadataInput("Screenshot path", "screenshot_path", metadata.screenshot_path, "text", "media/game/screenshot.png")}
        ${metadataInput("Logo path", "logo_path", metadata.logo_path, "text", "media/game/logo.png")}
      </div>
      <details class="asset-upload-editor">
        <summary>Upload artwork/media asset</summary>
        <p class="muted">
          Upload a file under the configured asset root, then apply that relative path to a metadata field.
        </p>
        <label>
          <span>Destination asset path</span>
          <input id="asset-upload-path" type="text" placeholder="media/${escapeHtml(game.id)}/screenshot.png" />
        </label>
        <label>
          <span>Apply uploaded path to</span>
          <select id="asset-upload-field">
            <option value="screenshot_path">Screenshot path</option>
            <option value="marquee_path">Marquee path</option>
            <option value="artwork_path">Artwork path</option>
            <option value="logo_path">Logo path</option>
          </select>
        </label>
        <label>
          <span>File</span>
          <input id="asset-upload-file" type="file" />
        </label>
        <button id="upload-asset" type="button">Upload asset</button>
        <div id="asset-upload-result"></div>
      </details>
      <details class="player-slot-editor">
        <summary>Player slot metadata</summary>
        <p class="muted">
          Optional labels, character names, and per-player artwork for seat selection and future overlays.
        </p>
        <div class="player-slot-grid">
          ${renderPlayerSlotFields(metadata.player_slots || [], slotCount)}
        </div>
      </details>
      <div class="form-actions">
        <button type="submit">Save metadata</button>
        <button id="cancel-metadata-edit" type="button">Cancel</button>
      </div>
      <div id="metadata-result"></div>
    </form>
  `;
  const form = editor.querySelector("#metadata-form");
  form.addEventListener("submit", (event) => saveGameMetadata(event, game));
  editor.querySelector("#cancel-metadata-edit").addEventListener("click", () => {
    editor.classList.add("muted");
    editor.textContent = "Select a game to edit its metadata.";
  });
  editor.querySelector("#upload-asset").addEventListener("click", () => uploadAsset(form));
}

function metadataInput(label, name, value, type = "text", placeholder = "") {
  return `
    <label>
      <span>${escapeHtml(label)}</span>
      <input name="${escapeHtml(name)}" type="${escapeHtml(type)}" value="${escapeHtml(value ?? "")}" placeholder="${escapeHtml(placeholder)}" />
    </label>
  `;
}

function metadataTextarea(label, name, value) {
  return `
    <label>
      <span>${escapeHtml(label)}</span>
      <textarea name="${escapeHtml(name)}" rows="3">${escapeHtml(value ?? "")}</textarea>
    </label>
  `;
}

function renderPlayerSlotFields(playerSlots, slotCount) {
  const slotsByNumber = new Map(
    playerSlots.map((slot) => [Number(slot.player_number), slot]),
  );
  const count = Math.max(1, Math.min(16, Number(slotCount) || 1));
  return Array.from({ length: count }, (_, index) => {
    const playerNumber = index + 1;
    const slot = slotsByNumber.get(playerNumber) || {};
    return `
      <fieldset class="player-slot-card">
        <legend>Player ${playerNumber}</legend>
        <input type="hidden" name="slot_${playerNumber}_player_number" value="${playerNumber}" />
        ${metadataInput("Label", `slot_${playerNumber}_label`, slot.label, "text", `P${playerNumber}`)}
        ${metadataInput("Position", `slot_${playerNumber}_position`, slot.position, "text", `P${playerNumber}`)}
        ${metadataInput("Character", `slot_${playerNumber}_character`, slot.character, "text", "Character name")}
        ${metadataInput("Artwork path", `slot_${playerNumber}_artwork_path`, slot.artwork_path, "text", `media/game/p${playerNumber}.png`)}
      </fieldset>
    `;
  }).join("");
}

async function saveGameMetadata(event, game) {
  event.preventDefault();
  const form = event.currentTarget;
  const result = form.querySelector("#metadata-result");
  const metadata = metadataFromForm(form);
  result.innerHTML = `<p class="pill warn">Saving metadata...</p>`;
  try {
    const updated = await putJson(`/api/v1/games/${game.id}/metadata`, { metadata });
    const index = state.games.findIndex((candidate) => candidate.id === updated.id);
    if (index >= 0) {
      state.games[index] = updated;
    }
    renderGames();
    renderGameEditor(updated);
    document.querySelector("#metadata-result").innerHTML = `
      <div class="item">
        <h3>Metadata saved</h3>
        <span class="pill good">${escapeHtml(updated.id)}</span>
        <p class="muted">The catalog view has been refreshed with the latest metadata.</p>
      </div>
    `;
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

async function uploadAsset(form) {
  const path = form.querySelector("#asset-upload-path").value.trim();
  const field = form.querySelector("#asset-upload-field").value;
  const fileInput = form.querySelector("#asset-upload-file");
  const result = form.querySelector("#asset-upload-result");
  const file = fileInput.files?.[0];
  if (!path || !file) {
    result.innerHTML = `<p class="pill bad">Choose a destination path and file.</p>`;
    return;
  }
  result.innerHTML = `<p class="pill warn">Uploading asset...</p>`;
  try {
    await putRaw(`/api/v1/assets/${encodeAssetPath(path)}`, file);
    form.elements[field].value = path;
    result.innerHTML = `
      <div class="item">
        <h3>Asset uploaded</h3>
        <span class="pill good">${escapeHtml(path)}</span>
        <p class="muted">The path was applied to ${escapeHtml(field)}. Save metadata to keep the reference.</p>
      </div>
    `;
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

function encodeAssetPath(path) {
  return path
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

function metadataFromForm(form) {
  const data = new FormData(form);
  const metadata = {};
  for (const field of [
    "sort_title",
    "description",
    "genre",
    "manufacturer",
    "artwork_path",
    "marquee_path",
    "screenshot_path",
    "logo_path",
    "control_notes",
  ]) {
    const value = String(data.get(field) || "").trim();
    if (value) {
      metadata[field] = value;
    }
  }
  for (const field of ["release_year", "player_count"]) {
    const value = String(data.get(field) || "").trim();
    if (value) {
      metadata[field] = Number(value);
    }
  }
  metadata.player_slots = playerSlotsFromForm(data, metadata.player_count);
  return metadata;
}

function playerSlotsFromForm(data, playerCount) {
  const count = Math.max(1, Math.min(16, Number(playerCount) || 16));
  const slots = [];
  for (let playerNumber = 1; playerNumber <= count; playerNumber += 1) {
    const slot = { player_number: playerNumber };
    for (const field of ["label", "position", "character", "artwork_path"]) {
      const value = String(data.get(`slot_${playerNumber}_${field}`) || "").trim();
      if (value) {
        slot[field] = value;
      }
    }
    if (slot.label || slot.position || slot.character || slot.artwork_path) {
      slots.push(slot);
    }
  }
  return slots;
}

function renderProduction(session) {
  state.selectedSession = session;
  const fullSession = state.sessions.find((candidate) => candidate.id === session.id);
  const mediaPort = fullSession?.connection_grant?.media_udp_port || "unknown";
  const inputPort = fullSession?.connection_grant?.input_udp_port || "unknown";
  const container = document.querySelector("#production");
  container.innerHTML = `
    <div class="item">
      <h3>${escapeHtml(session.display_name || session.game_id)}</h3>
      <p>
        Session <code>${escapeHtml(session.id)}</code> is running on
        <code>${escapeHtml(session.runtime_host_id || "")}</code>.
      </p>
      <p class="muted">
        Create a production spectator grant to reserve a separate media port for OBS
        or another receiver. This does not reserve a player slot.
      </p>
      <span class="pill">player media ${escapeHtml(String(mediaPort))}</span>
      <span class="pill">input ${escapeHtml(String(inputPort))}</span>
      <button id="create-spectator-grant" type="button">Create production spectator feed</button>
      <button id="stop-session" class="danger" type="button">Stop session</button>
      <div id="spectator-grant-result"></div>
      <div id="session-action-result"></div>
      <div class="producer-notes">
        <label>
          <span>Producer notes</span>
          <textarea id="producer-notes" rows="6" placeholder="Match notes, winner, stream callouts, technical observations...">${escapeHtml(producerNotesForSession(session.id))}</textarea>
        </label>
        <div class="form-actions">
          <button id="save-producer-notes" type="button">Save notes</button>
          <button id="clear-producer-notes" type="button">Clear notes</button>
        </div>
        <div id="producer-notes-result"></div>
      </div>
    </div>
  `;
  document
    .querySelector("#create-spectator-grant")
    .addEventListener("click", () => createProductionSpectatorGrant(session));
  document
    .querySelector("#stop-session")
    .addEventListener("click", () => stopSession(session));
  document
    .querySelector("#save-producer-notes")
    .addEventListener("click", () => saveProducerNotes(session.id));
  document
    .querySelector("#clear-producer-notes")
    .addEventListener("click", () => clearProducerNotes(session.id));
}

function producerNotesForSession(sessionId) {
  return localStorage.getItem(producerNotesKey(sessionId)) || "";
}

function producerNotesKey(sessionId) {
  return `fourplay.producerNotes.${sessionId}`;
}

function saveProducerNotes(sessionId) {
  const notes = document.querySelector("#producer-notes").value.trim();
  localStorage.setItem(producerNotesKey(sessionId), notes);
  document.querySelector("#producer-notes-result").innerHTML = `
    <p class="pill good">Notes saved locally</p>
  `;
}

function clearProducerNotes(sessionId) {
  localStorage.removeItem(producerNotesKey(sessionId));
  document.querySelector("#producer-notes").value = "";
  document.querySelector("#producer-notes-result").innerHTML = `
    <p class="pill warn">Notes cleared locally</p>
  `;
}

async function createProductionSpectatorGrant(session) {
  const destination = producerDestinationInput.value.trim();
  state.producerDestination = destination;
  localStorage.setItem("fourplay.producerDestination", destination);
  const result = document.querySelector("#spectator-grant-result");
  if (!destination) {
    result.innerHTML = `<p class="pill bad">Producer IP is required.</p>`;
    return;
  }
  try {
    const grant = await postJson(`/api/v1/sessions/${session.id}/spectators`, {
      seat_id: "admin-producer",
      destination_address: destination,
    });
    rememberProductionGrant(grant);
    renderProductionGrantResult(result, grant);
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

function rememberProductionGrant(grant) {
  state.productionGrants = [
    grant,
    ...state.productionGrants.filter((candidate) => candidate.id !== grant.id),
  ].slice(0, 10);
}

function renderProductionGrantResult(result, grant) {
  const udpUrl = `udp://0.0.0.0:${grant.media_udp_port}?fifo_size=1000000&overrun_nonfatal=1`;
  const ffplay = `ffplay -f mpegts -fflags nobuffer -flags low_delay -framedrop -probesize 32768 -analyzeduration 0 "${udpUrl}"`;
  const captureUrl = productionCaptureUrl(grant);
  result.innerHTML = `
    <div class="item">
      <h3>Production spectator grant created</h3>
      <span class="pill good">grant ${escapeHtml(grant.id)}</span>
      <span class="pill">runtime ${escapeHtml(grant.runtime_host_id)}</span>
      <span class="pill">media ${escapeHtml(String(grant.media_udp_port))}</span>
      <p>Receiver URL:</p>
      <pre class="command">${escapeHtml(udpUrl)}</pre>
      <p>OBS Media Source:</p>
      <ol class="muted">
        <li>Add a <strong>Media Source</strong>.</li>
        <li>Uncheck <strong>Local File</strong>.</li>
        <li>Paste the receiver URL into <strong>Input</strong>.</li>
        <li>Enable <strong>Restart playback when source becomes active</strong>.</li>
      </ol>
      <p>ffplay command:</p>
      <pre class="command">${escapeHtml(ffplay)}</pre>
      <p>
        <a class="button-link" href="${escapeHtml(captureUrl)}" target="_blank" rel="noreferrer">
          Open clean production capture helper
        </a>
      </p>
      <button id="release-spectator-grant" type="button">Release production spectator feed</button>
    </div>
  `;
  result
    .querySelector("#release-spectator-grant")
    .addEventListener("click", () => releaseProductionSpectatorGrant(grant));
}

function productionCaptureUrl(grant) {
  const selected = state.selectedSession;
  const params = new URLSearchParams({
    session: grant.session_id,
    grant: grant.id,
    port: String(grant.media_udp_port),
    game: selected?.display_name || selected?.game_id || "Production Feed",
  });
  return `/admin/capture?${params.toString()}`;
}

async function releaseProductionSpectatorGrant(grant) {
  const result = document.querySelector("#spectator-grant-result");
  try {
    await deleteJson(`/api/v1/sessions/${grant.session_id}/spectators/${grant.id}`, {
      seat_id: grant.seat_id || "admin-producer",
    });
    state.productionGrants = state.productionGrants.filter(
      (candidate) => candidate.id !== grant.id,
    );
    result.innerHTML = `
      <div class="item">
        <h3>Production spectator feed released</h3>
        <span class="pill good">grant ${escapeHtml(grant.id)}</span>
        <span class="pill">media ${escapeHtml(String(grant.media_udp_port))}</span>
        <p class="muted">The OBS/producer media port has been returned to the runtime host pool.</p>
      </div>
    `;
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

async function stopSession(session) {
  const result = document.querySelector("#session-action-result");
  const label = session.display_name || session.game_id || session.id;
  const confirmed = window.confirm(
    `Stop ${label}?\n\nThis will end gameplay for every connected player and spectator in this session.`,
  );
  if (!confirmed) {
    return;
  }
  result.innerHTML = `<p class="pill warn">Stopping ${escapeHtml(label)}...</p>`;
  try {
    const updated = await postJson(`/api/v1/sessions/${session.id}/stop`);
    result.innerHTML = `
      <div class="item">
        <h3>Session stop requested</h3>
        <span class="pill warn">${escapeHtml(updated.state || "stopping")}</span>
        <span class="pill">${escapeHtml(updated.id || session.id)}</span>
        <p class="muted">The runtime host will shut down the emulator and release session resources.</p>
      </div>
    `;
    await refresh();
  } catch (error) {
    result.innerHTML = `<p class="pill bad">${escapeHtml(String(error))}</p>`;
  }
}

async function postJson(path, body) {
  return sendJson("POST", path, body);
}

async function putJson(path, body) {
  return sendJson("PUT", path, body);
}

async function putRaw(path, body) {
  const response = await fetch(path, {
    method: "PUT",
    headers: authHeaders(),
    body,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${text}`);
  }
}

async function deleteJson(path, body) {
  return sendJson("DELETE", path, body);
}

async function sendJson(method, path, body) {
  const response = await fetch(path, {
    method,
    headers: {
      ...authHeaders(),
      "Content-Type": "application/json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${text}`);
  }
  return response.json();
}

function renderSnapshot(snapshot) {
  document.querySelector("#snapshot").textContent = JSON.stringify(snapshot, null, 2);
}

function replaceChildren(container, children) {
  container.classList.remove("muted");
  container.replaceChildren(...children);
  if (children.length === 0) {
    const empty = document.querySelector("#empty-template").content.cloneNode(true);
    container.classList.add("muted");
    container.replaceChildren(empty);
  }
}

function item() {
  const element = document.createElement("div");
  element.className = "item";
  return element;
}

function setText(selector, value) {
  document.querySelector(selector).textContent = value;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

refresh();
