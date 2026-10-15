const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs"), vm = require("node:vm");
const V2 = require("../control-plane/server/static/client-v2/experience.js");

test("V2 API mapping stays same-origin, scoped, and excludes query credentials", () => {
  assert.equal(V2.apiUrl("/api/v1/games", "http://arcade"), "/client-v2/api/games");
  assert.equal(V2.apiUrl("http://arcade/api/v1/sessions/x/webrtc/offer", "http://arcade"), "/client-v2/api/sessions/x/webrtc/offer");
  for (const path of ["http://evil/api/v1/games", "/admin", "/api/v1/games?token=secret", "/api/v1/games#secret", "/client-v2/api/games"]) {
    assert.throws(() => V2.apiUrl(path, "http://arcade"));
  }
});
test("launch waits for active media, fails explicitly and has a bounded deadline", async () => {
  const active = { id: "s1", state: "active", connection_grant: { webrtc_signaling: { offer_path: "/api/v1/sessions/s1/webrtc/offer" } } };
  let now = 0, requests = 0;
  const result = await V2.waitForSession({ id: "s1", state: "allocated" }, async (id) => {
    assert.equal(id, "s1"); return ++requests === 1 ? { id, state: "launching" } : active;
  }, async (ms) => { now += ms; }, 2000, () => now);
  assert.equal(result, active); assert.equal(requests, 2);
  for (const state of ["launch_failed", "runtime_lost", "allocation_failed", "stopped", "stopping", "terminated", "unhealthy"]) {
    await assert.rejects(V2.waitForSession({ state }, () => assert.fail(), () => assert.fail()), /could not start/);
  }
  now = 0;
  await assert.rejects(V2.waitForSession({ id: "s1", state: "active" }, async () => ({ state: "active" }), async (ms) => { now += ms; }, 1000, () => now), /taking too long/);
  await assert.rejects(V2.waitForSession({ id: "s1" }, async () => { throw Error("offline"); }, async () => {}), /offline/);
});

function launchSetup() {
  const elements = new Map(), calls = [], viewers = [];
  const element = (id) => { if (!elements.has(id)) elements.set(id, { innerHTML: "", textContent: "" }); return elements.get(id); };
  const active = { id: "s1", state: "active", runtime_profile: { buttons_per_player: 6 }, connection_grant: { webrtc_signaling: { offer_path: "/api/v1/sessions/s1/webrtc/offer" } } };
  const context = vm.createContext({ URL, state: { seatId: "auto-seat", destinationIp: "192.0.2.10" },
    window: { FourPlayV2: { ...V2 }, location: { origin: "http://arcade" }, setTimeout: (fn) => queueMicrotask(fn) },
    document: { querySelector: element }, syncSettingsFromInputs() {}, rememberRuntime() {}, publicPlayerHandoff: () => ({}),
    clientApi: { startGame: async (...args) => { calls.push(args); return { id: "s1", state: "allocated" }; } },
    fetchJson: async () => active, startBrowserMedia: async (button) => viewers.push(button.dataset),
    refresh: async () => {}, escapeHtml: (s) => String(s), setText: (id, text) => { element(id).textContent = text; },
  });
  const source = fs.readFileSync("control-plane/server/static/client/app.js", "utf8");
  vm.runInContext(source.slice(source.indexOf("async function startGame("), source.indexOf("function syncSettingsFromInputs(")), context);
  return { context, calls, viewers, elements };
}
test("shipped Start Game creates once and automatically opens browser media with the game profile", async () => {
  const s = launchSetup();
  let resolve; s.context.clientApi.startGame = async (...args) => { s.calls.push(args); return new Promise((done) => { resolve = done; }); };
  const first = s.context.startGame({ id: "kinst" });
  await s.context.startGame({ id: "tmnt" }); assert.equal(s.calls.length, 1);
  resolve({ id: "s1", state: "allocated" }); await first;
  assert.equal(s.viewers.length, 1); assert.equal(s.viewers[0].browserMediaButtons, "6");
  assert.equal(s.viewers[0].browserMediaRole, "player"); assert.equal(s.viewers[0].browserMediaPlayer, "1");
  assert.equal(s.context.window.FourPlayV2.launching, false);
  assert.ok(!s.elements.get("#client-action-result").innerHTML.includes("ffplay"));
});
test("failed startup does not open a viewer or leave the launch lock held", async () => {
  const s = launchSetup(); s.context.fetchJson = async () => ({ state: "launch_failed" });
  await s.context.startGame({ id: "bad-rom" });
  assert.equal(s.viewers.length, 0); assert.equal(s.context.window.FourPlayV2.launching, false);
  assert.match(s.elements.get("#client-action-result").innerHTML, /could not start/);
});
test("cover-flow offsets are bounded around the selected cover", () => {
  assert.equal(V2.coverOffset(5, 5), 0); assert.equal(V2.coverOffset(0, 5), -3); assert.equal(V2.coverOffset(12, 5), 3);
});

test("shipped join and spectate open the viewer automatically with the correct role", async () => {
  const s = launchSetup(), actions = [];
  const active = { id: "s1", state: "active", runtime_profile: { buttons_per_player: 2 }, connection_grant: { webrtc_signaling: { offer_path: "/api/v1/sessions/s1/webrtc/offer" } } };
  s.context.clientApi = {
    reservePlayerSlot: async () => actions.push("reserve"),
    connectPlayerSlot: async () => { actions.push("connect"); return active; },
    spectate: async () => ({ id: "viewer-grant", webrtc_signaling: active.connection_grant.webrtc_signaling }),
  };
  s.context.publicSpectatorHandoff = () => ({});
  const source = fs.readFileSync("control-plane/server/static/client/app.js", "utf8");
  vm.runInContext(source.slice(source.indexOf("async function joinPlayerSlot("), source.indexOf("function runtimePills(")), s.context);
  await s.context.joinPlayerSlot(active, 2, false);
  assert.deepEqual(actions, ["reserve", "connect"]);
  assert.equal(s.viewers[0].browserMediaPlayer, "2"); assert.equal(s.viewers[0].browserMediaButtons, "2");
  await s.context.spectateSession(active);
  assert.equal(s.viewers[1].browserMediaRole, "spectator"); assert.equal(s.viewers[1].browserMediaGrant, "viewer-grant");
  assert.equal(s.context.window.FourPlayV2.launching, false);
});

test("cover artwork stays bounded to the selected neighborhood and releases old cards", () => {
  const active = new Set();
  const root = {
    location: { origin: "http://arcade" }, URL, AbortController,
    document: { body: { dataset: { clientVersion: "2" }, append() {}, classList: { toggle() {} } },
      createElement: () => ({ setAttribute() {}, remove() { this.removed = true; }, removeAttribute() {} }) },
    FourPlayCarousel: { ArtworkLoader: class {
      constructor() { active.add(this); }
      clear() { active.delete(this); }
      load(game, headers, apply) { assert.equal(headers["X-4Play-Client"], "v2"); apply("blob:test"); }
    } },
  };
  const context = vm.createContext({ window: root, URL });
  vm.runInContext(fs.readFileSync("control-plane/server/static/client-v2/experience.js", "utf8"), context);
  const cards = Array.from({ length: 20 }, () => ({ style: { setProperty() {} }, image: null,
    prepend(image) { this.image = image; }, querySelector() { return this.image; } }));
  const selection = { index: 0, games: cards.map((_, id) => ({ id })) };
  root.FourPlayV2.updateCovers(selection, cards); assert.equal(active.size, 4);
  selection.index = 10; root.FourPlayV2.updateCovers(selection, cards); assert.equal(active.size, 7);
  assert.equal(cards[0].image.removed, true);
  root.FourPlayV2.updateCovers({ index: 0, games: [] }, []); assert.equal(active.size, 0);
});
