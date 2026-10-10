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

function renderGame(game) {
  const element = item();
  const metadata = game.metadata || {};
  element.innerHTML = `
    <h3>${escapeHtml(game.display_name || game.id)}</h3>
    <span class="pill">${escapeHtml(game.id)}</span>
    <span class="pill">${escapeHtml(metadata.genre || "unknown genre")}</span>
    <span class="pill">${escapeHtml(String(metadata.player_count || game.availability?.[0]?.profile?.max_players || "?"))} players</span>
    <p class="muted">${escapeHtml(metadata.description || "No description yet.")}</p>
  `;
  return element;
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
      <div id="spectator-grant-result"></div>
    </div>
  `;
  document
    .querySelector("#create-spectator-grant")
    .addEventListener("click", () => createProductionSpectatorGrant(session));
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

async function postJson(path, body) {
  return sendJson("POST", path, body);
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
    body: JSON.stringify(body),
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
