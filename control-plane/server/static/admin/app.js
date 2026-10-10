const tokenInput = document.querySelector("#api-token");
const saveTokenButton = document.querySelector("#save-token");
const refreshButton = document.querySelector("#refresh");
const gameFilter = document.querySelector("#game-filter");

const state = {
  token: localStorage.getItem("fourplay.adminToken") || "",
  games: [],
  hosts: [],
  sessions: [],
  activeSessions: [],
};

tokenInput.value = state.token;

saveTokenButton.addEventListener("click", () => {
  state.token = tokenInput.value.trim();
  localStorage.setItem("fourplay.adminToken", state.token);
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
  const container = document.querySelector("#production");
  container.innerHTML = `
    <div class="item">
      <h3>${escapeHtml(session.display_name || session.game_id)}</h3>
      <p>
        Session <code>${escapeHtml(session.id)}</code> is running on
        <code>${escapeHtml(session.runtime_host_id || "")}</code>.
      </p>
      <p class="muted">
        Production v0: use this panel to identify the match/session. Next step is
        creating a spectator grant and opening a clean OBS capture window.
      </p>
      <span class="pill">media ${escapeHtml(String(session.media_udp_port || "unknown"))}</span>
      <span class="pill">input ${escapeHtml(String(session.input_udp_port || "unknown"))}</span>
    </div>
  `;
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
