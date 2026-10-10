const state = {
  viewModel: null,
  selectedGameIndex: 0,
  overlayTimer: null,
};

const elements = {
  games: document.querySelector("#games"),
  sessions: document.querySelector("#sessions"),
  refresh: document.querySelector("#refresh"),
  runtimeStatus: document.querySelector("#runtime-status"),
  overlay: document.querySelector("#actions-overlay"),
  overlayTitle: document.querySelector("#overlay-title"),
  overlayActions: document.querySelector("#overlay-actions"),
};

elements.refresh.addEventListener("click", () => refresh());
document.addEventListener("keydown", (event) => {
  if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", " "].includes(event.key)) {
    showOverlay("Navigation active", ["Start", "Join", "Spectate"]);
  }
});

refresh();

async function refresh() {
  const response = await nativeCommand({ type: "refresh" });
  if (response.type !== "view_model") {
    throw new Error(`Unexpected refresh response: ${response.type}`);
  }
  state.viewModel = response.payload;
  render();
}

async function nativeCommand(command) {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke("native_command", { command });
  }
  if (window.__FOURPLAY_NATIVE__?.command) {
    return window.__FOURPLAY_NATIVE__.command(command);
  }
  return mockNativeCommand(command);
}

function render() {
  const view = state.viewModel;
  elements.runtimeStatus.textContent = runtimeLabel(view.current_runtime);
  elements.games.replaceChildren(...view.games.map((game) => gameCard(game)));
  elements.sessions.replaceChildren(...view.active_sessions.map((session) => sessionCard(session)));
}

function gameCard(game) {
  const card = document.createElement("article");
  card.className = "card";
  card.tabIndex = 0;
  card.innerHTML = `
    <h3>${escapeHtml(game.display_name)}</h3>
    <div class="tags">
      <span class="tag">${escapeHtml(game.id)}</span>
      ${game.genre ? `<span class="tag">${escapeHtml(game.genre)}</span>` : ""}
      ${game.player_count ? `<span class="tag">${game.player_count} players</span>` : ""}
    </div>
    <p>${escapeHtml(summaryLine(game))}</p>
    <div class="actions">
      <button data-action="start">Start</button>
    </div>
  `;
  card.querySelector("[data-action='start']").addEventListener("click", async () => {
    await nativeCommand({ type: "start_game", game_id: game.id });
    showOverlay(`Starting ${game.display_name}`, ["Launching media", "Input ready"]);
    await refresh();
  });
  return card;
}

function sessionCard(session) {
  const card = document.createElement("article");
  card.className = "card";
  const joinButtons = session.joinable_slots
    .map(
      (slot) =>
        `<button data-join="${slot.player_number}">Join P${slot.player_number}</button>`,
    )
    .join("");
  card.innerHTML = `
    <h3>${escapeHtml(session.display_name)}</h3>
    <div class="tags">
      <span class="tag">${escapeHtml(session.state)}</span>
      <span class="tag">${session.active_spectator_count} spectators</span>
    </div>
    <p>${session.joinable_slots.length} open/rejoinable slots</p>
    <div class="actions">
      ${joinButtons}
      ${session.can_spectate ? "<button data-spectate>Spectate</button>" : ""}
    </div>
  `;
  for (const button of card.querySelectorAll("[data-join]")) {
    button.addEventListener("click", async () => {
      await nativeCommand({
        type: "join_session",
        session_id: session.id,
        player_number: Number(button.dataset.join),
      });
      showOverlay(`Joined ${session.display_name}`, ["Playing", "Esc / menu to leave"]);
      await refresh();
    });
  }
  card.querySelector("[data-spectate]")?.addEventListener("click", async () => {
    await nativeCommand({ type: "spectate_session", session_id: session.id });
    showOverlay(`Spectating ${session.display_name}`, ["Join open slot", "Stop viewing"]);
    await refresh();
  });
  return card;
}

function showOverlay(title, actions) {
  elements.overlayTitle.textContent = title;
  elements.overlayActions.replaceChildren(
    ...actions.map((action) => {
      const chip = document.createElement("span");
      chip.className = "tag";
      chip.textContent = action;
      return chip;
    }),
  );
  elements.overlay.classList.remove("hidden");
  clearTimeout(state.overlayTimer);
  state.overlayTimer = setTimeout(() => elements.overlay.classList.add("hidden"), 2600);
}

function runtimeLabel(runtime) {
  if (!runtime) {
    return "Browsing";
  }
  const player = runtime.player_number ? ` P${runtime.player_number}` : "";
  return `${runtime.mode}${player} · ${runtime.game_id}`;
}

function summaryLine(game) {
  return [game.release_year, game.manufacturer].filter(Boolean).join(" · ") || "Ready to launch";
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

async function mockNativeCommand(command) {
  if (command.type !== "refresh") {
    console.info("Mock native command", command);
    return { type: "ack" };
  }
  return {
    type: "view_model",
    payload: {
      current_runtime: null,
      games: [
        {
          id: "tmnt",
          display_name: "Teenage Mutant Ninja Turtles",
          genre: "Beat 'em up",
          release_year: 1989,
          manufacturer: "Konami",
          player_count: 4,
          screenshot_path: null,
          marquee_path: null,
          logo_path: null,
          available: true,
        },
        {
          id: "kinst",
          display_name: "Killer Instinct",
          genre: "Fighting",
          release_year: 1994,
          manufacturer: "Rare / Midway",
          player_count: 2,
          screenshot_path: null,
          marquee_path: null,
          logo_path: null,
          available: true,
        },
      ],
      active_sessions: [
        {
          id: "session-1",
          game_id: "tmnt",
          display_name: "Teenage Mutant Ninja Turtles",
          runtime_host_id: "reference-linux",
          state: "active",
          max_players: 4,
          joinable_slots: [{ player_number: 2, state: "open" }],
          player_slots: [],
          can_spectate: true,
          active_spectator_count: 1,
          preview_asset_path: null,
        },
      ],
    },
  };
}
