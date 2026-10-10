const state = {
  viewModel: null,
  selectedGameIndex: 0,
  overlayTimer: null,
  heldKeys: new Set(),
  lastAxis: { x: 0, y: 0 },
  inputEnabled: false,
  lastGamepad: {
    buttons: new Map(),
    axis: { x: 0, y: 0 },
  },
};

const elements = {
  games: document.querySelector("#games"),
  sessions: document.querySelector("#sessions"),
  refresh: document.querySelector("#refresh"),
  runtimeStatus: document.querySelector("#runtime-status"),
  overlay: document.querySelector("#actions-overlay"),
  overlayTitle: document.querySelector("#overlay-title"),
  overlayActions: document.querySelector("#overlay-actions"),
  message: document.querySelector("#message"),
};

elements.refresh.addEventListener("click", () => runUiAction(() => refresh()));
document.addEventListener("keydown", (event) => handleKey(event, true));
document.addEventListener("keyup", (event) => handleKey(event, false));
window.addEventListener("blur", () => neutralizeInput("Window focus lost"));
window.addEventListener("gamepadconnected", (event) => {
  showOverlay(`Gamepad connected: ${event.gamepad.id}`, ["Left stick/D-pad move", "Buttons mapped"]);
});
window.addEventListener("gamepaddisconnected", () => {
  neutralizeInput("Gamepad disconnected");
  state.lastGamepad.buttons.clear();
  state.lastGamepad.axis = { x: 0, y: 0 };
  showOverlay("Gamepad disconnected", ["Input neutralized"]);
});

runUiAction(() => refresh());
requestAnimationFrame(pollGamepads);

async function refresh() {
  setMessage("Refreshing seat state...");
  const response = await nativeCommand({ type: "refresh" });
  if (response.type !== "view_model") {
    throw new Error(`Unexpected refresh response: ${response.type}`);
  }
  state.viewModel = response.payload;
  render();
  setMessage("Seat state refreshed.");
}

async function nativeCommand(command) {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke("native_command", { command });
  }
  if (window.__FOURPLAY_NATIVE__?.command) {
    return window.__FOURPLAY_NATIVE__.command(command);
  }
  if (location.protocol === "http:" || location.protocol === "https:") {
    try {
      const response = await fetch("./native-command", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(command),
      });
      if (response.ok) {
        return response.json();
      }
      throw new Error(await response.text());
    } catch (error) {
      if (location.hostname === "" || location.protocol === "file:") {
        console.warn("Native bridge unavailable; using mock data", error);
      } else {
        throw error;
      }
    }
  }
  return mockNativeCommand(command);
}

function render() {
  const view = state.viewModel;
  state.inputEnabled = view.current_runtime?.mode === "player";
  elements.runtimeStatus.textContent = runtimeLabel(view.current_runtime);
  elements.games.replaceChildren(...view.games.map((game) => gameCard(game)));
  elements.sessions.replaceChildren(...view.active_sessions.map((session) => sessionCard(session)));
  renderRuntimeActions(view.current_runtime);
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
  card.querySelector("[data-action='start']").addEventListener("click", () => runUiAction(async () => {
    await nativeCommand({ type: "start_game", game_id: game.id });
    showOverlay(`Starting ${game.display_name}`, ["Launching media", "Input ready"]);
    await refresh();
  }));
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
    button.addEventListener("click", () => runUiAction(async () => {
      await nativeCommand({
        type: "join_session",
        session_id: session.id,
        player_number: Number(button.dataset.join),
      });
      showOverlay(`Joined ${session.display_name}`, ["Playing", "Esc / menu to leave"]);
      await refresh();
    }));
  }
  card.querySelector("[data-spectate]")?.addEventListener("click", () => runUiAction(async () => {
    await nativeCommand({ type: "spectate_session", session_id: session.id });
    showOverlay(`Spectating ${session.display_name}`, ["Join open slot", "Stop viewing"]);
    await refresh();
  }));
  return card;
}

function renderRuntimeActions(runtime) {
  if (!runtime) {
    return;
  }
  const actions = runtime.mode === "player"
    ? ["WASD/gamepad move", "J/K/L/M/,/. buttons", "1 coin", "2 start", "Esc stop"]
    : ["Spectating", "Join an open slot to play", "Esc stop local feed"];
  showOverlay(runtimeLabel(runtime), actions);
}

async function handleKey(event, pressed) {
  if (event.repeat && pressed) {
    return;
  }

  if (event.code === "Escape" && pressed) {
    event.preventDefault();
    const command = state.viewModel?.current_runtime?.mode === "player"
      ? { type: "send_stop_input" }
      : { type: "stop_runtime" };
    await runUiAction(() => nativeCommand(command));
    state.heldKeys.clear();
    state.lastAxis = { x: 0, y: 0 };
    showOverlay("Stop sent", ["Runtime stop requested", "Refreshing"]);
    setTimeout(() => refresh(), 500);
    return;
  }

  if (!state.inputEnabled) {
    if (pressed && ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "Space"].includes(event.code)) {
      showOverlay("Navigation active", ["Start", "Join", "Spectate"]);
    }
    return;
  }

  const button = buttonForKey(event.code);
  if (button) {
    event.preventDefault();
    await setHeldKey(event.code, pressed);
    await runUiAction(() => nativeCommand({ type: "set_button", button, pressed }), false);
    showOverlay("Input active", ["WASD move", "J/K/L/M/,/. attack", "1 coin", "2 start"]);
    return;
  }

  if (isAxisKey(event.code)) {
    event.preventDefault();
    await setHeldKey(event.code, pressed);
    const axis = axisFromHeldKeys();
    if (axis.x !== state.lastAxis.x || axis.y !== state.lastAxis.y) {
      state.lastAxis = axis;
      await runUiAction(() => nativeCommand({ type: "set_axis", axis_x: axis.x, axis_y: axis.y }), false);
    }
    showOverlay("Input active", ["WASD move", "Diagonals supported"]);
  }
}

async function pollGamepads() {
  try {
    if (state.inputEnabled && navigator.getGamepads) {
      const gamepad = Array.from(navigator.getGamepads()).find(Boolean);
      if (gamepad) {
        await forwardGamepad(gamepad);
      }
    }
  } finally {
    requestAnimationFrame(pollGamepads);
  }
}

async function forwardGamepad(gamepad) {
  const axis = axisFromGamepad(gamepad);
  if (axis.x !== state.lastGamepad.axis.x || axis.y !== state.lastGamepad.axis.y) {
    state.lastGamepad.axis = axis;
    await runUiAction(() => nativeCommand({ type: "set_axis", axis_x: axis.x, axis_y: axis.y }), false);
  }

  for (const [index, button] of gamepadButtonMap()) {
    const pressed = Boolean(gamepad.buttons[index]?.pressed);
    if (state.lastGamepad.buttons.get(index) !== pressed) {
      state.lastGamepad.buttons.set(index, pressed);
      await runUiAction(() => nativeCommand({ type: "set_button", button, pressed }), false);
    }
  }
}

function axisFromGamepad(gamepad) {
  const leftStickX = axisDirection(gamepad.axes[0] ?? 0);
  const leftStickY = axisDirection(gamepad.axes[1] ?? 0);
  const dpadX = (gamepad.buttons[15]?.pressed ? 1 : 0) - (gamepad.buttons[14]?.pressed ? 1 : 0);
  const dpadY = (gamepad.buttons[13]?.pressed ? 1 : 0) - (gamepad.buttons[12]?.pressed ? 1 : 0);
  return {
    x: dpadX || leftStickX,
    y: dpadY || leftStickY,
  };
}

function axisDirection(value) {
  if (value <= -0.35) {
    return -1;
  }
  if (value >= 0.35) {
    return 1;
  }
  return 0;
}

function gamepadButtonMap() {
  return new Map([
    [0, "action1"],
    [1, "action2"],
    [2, "action3"],
    [3, "action4"],
    [4, "action5"],
    [5, "action6"],
    [8, "coin"],
    [9, "start"],
  ]);
}

async function setHeldKey(code, pressed) {
  if (pressed) {
    state.heldKeys.add(code);
  } else {
    state.heldKeys.delete(code);
  }
}

async function neutralizeInput(reason) {
  if (!state.inputEnabled || !hasTrackedInput()) {
    return;
  }
  state.heldKeys.clear();
  state.lastAxis = { x: 0, y: 0 };
  state.lastGamepad.buttons.clear();
  state.lastGamepad.axis = { x: 0, y: 0 };
  await runUiAction(() => nativeCommand({ type: "neutralize_input" }), false);
  showOverlay(reason, ["Input neutralized"]);
}

function hasTrackedInput() {
  return state.heldKeys.size > 0
    || state.lastGamepad.axis.x !== 0
    || state.lastGamepad.axis.y !== 0
    || Array.from(state.lastGamepad.buttons.values()).some(Boolean);
}

async function runUiAction(action, showSuccess = true) {
  try {
    await action();
    if (showSuccess) {
      clearMessageSoon();
    }
  } catch (error) {
    setMessage(errorMessage(error), true);
    showOverlay("Action failed", ["Check message banner", "Try refresh"]);
  }
}

function setMessage(message, isError = false) {
  elements.message.textContent = message;
  elements.message.classList.toggle("error", isError);
  elements.message.classList.remove("hidden");
}

function clearMessageSoon() {
  setTimeout(() => {
    elements.message.classList.add("hidden");
    elements.message.textContent = "";
    elements.message.classList.remove("error");
  }, 1800);
}

function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function buttonForKey(code) {
  return {
    KeyJ: "action1",
    KeyK: "action2",
    KeyL: "action3",
    KeyM: "action4",
    Comma: "action5",
    Period: "action6",
    Digit1: "coin",
    Digit2: "start",
  }[code] ?? null;
}

function isAxisKey(code) {
  return ["KeyW", "KeyA", "KeyS", "KeyD", "ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight"].includes(code);
}

function axisFromHeldKeys() {
  const left = state.heldKeys.has("KeyA") || state.heldKeys.has("ArrowLeft");
  const right = state.heldKeys.has("KeyD") || state.heldKeys.has("ArrowRight");
  const up = state.heldKeys.has("KeyW") || state.heldKeys.has("ArrowUp");
  const down = state.heldKeys.has("KeyS") || state.heldKeys.has("ArrowDown");
  return {
    x: left === right ? 0 : left ? -1 : 1,
    y: up === down ? 0 : up ? -1 : 1,
  };
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
  return `${runtime.mode}${player} - ${runtime.game_id}`;
}

function summaryLine(game) {
  return [game.release_year, game.manufacturer].filter(Boolean).join(" - ") || "Ready to launch";
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
