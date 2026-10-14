const assert = require("node:assert/strict");
const { test } = require("node:test");
const { SpectatorOverlay, acquirePlayer, joinableSlots } = require("../control-plane/server/static/client/spectator.js");

function menu() {
  const keyListeners = new Map(), timers = new Map();
  let nextTimer = 0, clicks = 0, shows = 0, pad = null;
  const video = {};
  const document = { hidden: false, activeElement: video, hasFocus: () => true,
    addEventListener: (name, callback) => keyListeners.set(name, callback),
    removeEventListener: (name) => keyListeners.delete(name) };
  const actions = [0, 1].map(() => ({ focus() { document.activeElement = this; }, click() { clicks++; } }));
  const panel = { inert: true, attrs: {}, classes: new Set(),
    classList: { add: (name) => panel.classes.add(name), remove: (name) => panel.classes.delete(name) },
    setAttribute: (name, value) => { panel.attrs[name] = value; },
    contains: (element) => actions.includes(element), querySelectorAll: () => actions,
    addEventListener() {}, removeEventListener() {} };
  const surface = { contains: (element) => element === video || actions.includes(element), addEventListener() {}, removeEventListener() {} };
  const env = { document, navigator: { getGamepads: () => pad ? [pad] : [] },
    setTimeout: (callback) => { timers.set(++nextTimer, callback); return nextTimer; }, clearTimeout: (id) => timers.delete(id),
    setInterval: () => 1, clearInterval() {} };
  const overlay = new SpectatorOverlay(surface, panel, { onShow: () => shows++ }, env);
  return { overlay, panel, document, actions, timers, keyListeners, clicks: () => clicks, shows: () => shows,
    hideTick: () => { const callback = timers.get(overlay.hideTimer); timers.delete(overlay.hideTimer); callback(); },
    pad: (buttons, axes = [0, 0]) => { pad = { mapping: "standard", axes, buttons: buttons.map((pressed) => ({ pressed })) }; },
  };
}

test("overlay wakes and hides after inactivity; stop removes timers and keyboard handler", () => {
  const s = menu(); s.overlay.start();
  assert.equal(s.panel.inert, false); assert.equal(s.shows(), 1);
  s.hideTick(); assert.equal(s.panel.inert, true); assert.equal(s.overlay.visible, false);
  s.keyListeners.get("keydown")({ code: "KeyJ" }); assert.equal(s.overlay.visible, true);
  assert.equal(s.clicks(), 0); // Spectator attack key never joins or forwards input.
  s.overlay.stop(); assert.equal(s.timers.size, 0); assert.equal(s.keyListeners.size, 0);
});

test("menu keeps deliberate keyboard focus and joining busy state visible", () => {
  const s = menu(); s.overlay.start();
  s.keyListeners.get("keydown")({ code: "ArrowRight", preventDefault() {} });
  assert.equal(s.document.activeElement, s.actions[0]);
  s.hideTick(); assert.equal(s.overlay.visible, true);
  s.document.activeElement = {}; s.overlay.setBusy(true);
  s.hideTick(); assert.equal(s.overlay.visible, true);
  s.overlay.setBusy(false); s.hideTick(); assert.equal(s.overlay.visible, false);
  s.keyListeners.get("keydown")({ code: "KeyJ" }); assert.equal(s.overlay.visible, false);
  s.overlay.stop();
});

test("gamepad wake cannot activate a slot; held buttons cannot repeat confirmation", () => {
  const s = menu(); s.overlay.start(); s.hideTick();
  s.actions[0].focus(); s.pad([true]); s.overlay.pollGamepad();
  assert.equal(s.clicks(), 0); assert.equal(s.overlay.visible, true);
  s.overlay.pollGamepad(); assert.equal(s.clicks(), 0);
  s.pad([false]); s.overlay.pollGamepad();
  s.pad([true]); s.overlay.pollGamepad(); assert.equal(s.clicks(), 1);
  s.overlay.pollGamepad(); assert.equal(s.clicks(), 1);
  s.document.hidden = true; s.pad([false]); s.overlay.pollGamepad(); s.pad([true]); s.overlay.pollGamepad();
  assert.equal(s.clicks(), 1); s.overlay.stop();
});

test("only open or same-seat disconnected/reserved slots are offered", () => {
  const slots = [ { player_number: 1, state: "occupied", seat_id: "a" },
    { player_number: 2, state: "open" }, { player_number: 3, state: "disconnected", seat_id: "a" },
    { player_number: 4, state: "reserved", seat_id: "other" } ];
  assert.deepEqual(joinableSlots({ player_slots: slots }, "a").map((s) => s.player_number), [2, 3]);
});

function acquisition() {
  const calls = [];
  const context = { role: "spectator", sessionId: "game", seatId: "seat-a" };
  const session = { id: "game", connection_grant: { token: "private-keep-in-memory" } };
  const api = Object.fromEntries(["reservePlayerSlot", "connectPlayerSlot", "disconnectPlayerSlot"].map((name) => [name,
    async (...args) => { calls.push([name, ...args]); return session; }]));
  return { context, calls, api, session };
}

test("joining acquires the selected player using the original viewer seat", async () => {
  const s = acquisition();
  assert.equal(await acquirePlayer(s.api, s.context, 2, () => true), s.session);
  assert.deepEqual(s.calls, [["reservePlayerSlot", "game", 2, "seat-a"], ["connectPlayerSlot", "game", 2, "seat-a"]]);
});

test("reservation conflict leaves the spectator untouched", async () => {
  const s = acquisition(); s.api.reservePlayerSlot = async () => { throw Error("slot taken"); };
  await assert.rejects(acquirePlayer(s.api, s.context, 2, () => true), /slot taken/);
  assert.equal(s.calls.length, 0);
});

test("connect failure and cancellation clean up only the acquired slot", async () => {
  const s = acquisition(); s.api.connectPlayerSlot = async () => { throw Error("connect failed"); };
  await assert.rejects(acquirePlayer(s.api, s.context, 2, () => true), /connect failed/);
  assert.deepEqual(s.calls.at(-1), ["disconnectPlayerSlot", "game", 2, "seat-a"]);
  const cancelled = acquisition(); let checks = 0;
  assert.equal(await acquirePlayer(cancelled.api, cancelled.context, 2, () => ++checks < 2), null);
  assert.deepEqual(cancelled.calls.map((c) => c[0]), ["reservePlayerSlot", "disconnectPlayerSlot"]);
  const duringConnect = acquisition(); let connectedChecks = 0;
  assert.equal(await acquirePlayer(duringConnect.api, duringConnect.context, 2, () => ++connectedChecks < 3), null);
  assert.deepEqual(duringConnect.calls.map((c) => c[0]), ["reservePlayerSlot", "connectPlayerSlot", "disconnectPlayerSlot"]);
});

test("failed cancellation cleanup explicitly reports the remaining player-slot obligation", async () => {
  const s = acquisition(); let checks = 0;
  s.api.disconnectPlayerSlot = async () => { throw Error("network unavailable"); };
  await assert.rejects(acquirePlayer(s.api, s.context, 2, () => ++checks < 2), /cleanup failed; use the session Leave action/);
});
