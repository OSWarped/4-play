/* Exercise the shipped client lifecycle with deterministic browser/API fakes. */
const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs");
const vm = require("node:vm");
const Spectator = require("../control-plane/server/static/client/spectator.js");

function setup() {
  const peers = [], inputs = [], monitors = [], calls = [];
  const bodyClasses = new Set();
  const state = { seatId: "seat-1", browserExitPending: false, browserMediaNegotiating: false, sessions: [] };
  const video = { focus() {}, pause() {}, srcObject: null, play: async () => {} };
  const player = { hidden: true, scrollIntoView() {} };
  const status = { textContent: "" }, reconnect = {};
  const inputStatus = { textContent: "" };
  const slots = { contains: () => false, replaceChildren() {}, querySelectorAll: () => [] };
  const elements = { "#spectator-player-slots": slots, "#spectator-actions": {}, "#spectator-action-status": {},
    "#browse-games": { focus() {} }, "#games": { scrollIntoView() {} } };
  class Peer {
    constructor() { this.listeners = {}; this.connectionState = "new"; this.iceGatheringState = "complete"; peers.push(this); }
    addTransceiver() {}
    addEventListener(name, callback) { this.listeners[name] = callback; }
    createDataChannel() {
      this.channel = { listeners: {}, addEventListener(name, callback) { this.listeners[name] = callback; } };
      return this.channel;
    }
    async createOffer() { return { sdp: "offer" }; }
    async setLocalDescription(offer) { this.localDescription = offer; }
    async setRemoteDescription() {}
    close() { this.connectionState = "closed"; }
  }
  class Input {
    constructor() { this.running = false; inputs.push(this); }
    bindTouch(panel, count) { this.touchCount = count; }
    start() { this.running = true; }
    stop() { this.running = false; }
  }
  class Monitor {
    constructor() { monitors.push(this); }
    start() {}
    stop() { this.stopped = true; }
  }
  const context = vm.createContext({
    state, URL, RTCPeerConnection: Peer, FourPlayBrowserInput: Input, FourPlayBrowserMediaMonitor: Monitor,
    MediaStream: class { constructor(tracks = []) { this.tracks = [...tracks]; } addTrack(track) { this.tracks.push(track); } getTracks() { return this.tracks; } },
    browserPlayer: player, browserMediaVideo: video, browserMediaState: status,
    reconnectBrowserMediaButton: reconnect, browserReceiverStats: { textContent: "" },
    document: { body: { classList: { add: (value) => bodyClasses.add(value), remove: (value) => bodyClasses.delete(value) } }, querySelector: (selector) => elements[selector] || inputStatus },
    window: { RTCPeerConnection: Peer, setInterval: () => 1, clearInterval() {}, location: { origin: "http://server" } },
    FourPlaySpectator: { ...Spectator, SpectatorOverlay: class { start() {} stop() {} setBusy() {} } },
    clientApi: {}, rememberRuntime() {}, clearLastRuntimeFor() {}, publicPlayerHandoff: () => ({ mode: "player" }), refresh: async () => {},
    syncSettingsFromInputs() {},
    postJson: async (url, body) => { calls.push({ url, body }); return { sdp_answer: "answer" }; },
  });
  const code = fs.readFileSync("control-plane/server/static/client/app.js", "utf8");
  vm.runInContext(code.slice(code.indexOf("async function startBrowserMedia("), code.indexOf("function renderLastRuntimeHint(")), context);
  const button = { dataset: { browserMediaUrl: "http://server/api/v1/sessions/game-1/webrtc/offer",
    browserMediaRole: "player", browserMediaPlayer: "2", browserMediaButtons: "2" } };
  return { context, state, peers, inputs, monitors, calls, video, status, reconnect, button, bodyClasses, player };
}

test("disconnect releases controls, reconnect preserves slot and closes old media", async () => {
  const s = setup(); await s.context.startBrowserMedia(s.button);
  const peer = s.peers[0]; peer.connectionState = "connected"; peer.listeners.connectionstatechange();
  peer.channel.listeners.open(); assert.equal(s.inputs[0].running, true);
  assert.equal(s.inputs[0].touchCount, "2");
  peer.connectionState = "disconnected"; peer.listeners.connectionstatechange();
  assert.equal(s.inputs[0].running, false);
  const retry = s.state.browserMediaRetry;
  await s.context.startBrowserMedia({ dataset: retry });
  assert.equal(peer.connectionState, "closed"); assert.equal(s.monitors[0].stopped, true);
  assert.equal(s.state.browserMediaContext.playerNumber, 2);
  assert.equal(s.inputs[1].touchCount, "2");
  assert.equal(s.calls.length, 2); // Only signaling; no disconnect/re-reserve request.
  const before = s.status.textContent;
  peer.listeners.track({ track: { kind: "video" } }); peer.listeners.connectionstatechange();
  assert.equal(s.video.srcObject, null);
  assert.equal(s.status.textContent, before);
});

test("audio-first delivery refreshes the video surface with both tracks and supports gesture resume", async () => {
  const s = setup(); await s.context.startBrowserMedia(s.button);
  assert.equal(s.video.srcObject, null);
  assert.equal(s.video.playsInline, true);
  assert.equal(s.bodyClasses.has("browser-playing"), true);
  const audio = { kind: "audio", stop() { this.stopped = true; } };
  const video = { kind: "video", stop() { this.stopped = true; } };
  s.peers[0].listeners.track({ track: audio });
  const first = s.video.srcObject;
  s.peers[0].listeners.track({ track: video });
  assert.notEqual(s.video.srcObject, first);
  assert.deepEqual(s.video.srcObject.getTracks(), [audio, video]);
  let played = 0; s.video.play = async () => { played++; };
  await s.context.resumeBrowserMedia(); assert.equal(played, 1);
  s.context.stopBrowserMedia(); assert.equal(audio.stopped, true); assert.equal(video.stopped, true);
  assert.equal(s.bodyClasses.has("browser-playing"), false);
  await s.context.resumeBrowserMedia(); assert.equal(played, 1);
});

test("failed offer keeps leave/retry context but releases media and input", async () => {
  const s = setup(); s.context.postJson = async () => { throw Error("publisher unavailable"); };
  await s.context.startBrowserMedia(s.button);
  assert.equal(s.state.browserMediaPeer, null); assert.equal(s.video.srcObject, null);
  assert.equal(s.state.browserMediaContext.sessionId, "game-1");
  assert.equal(s.reconnect.disabled, false); assert.equal(s.monitors[0].stopped, true);
  assert.match(s.status.textContent, /publisher unavailable/);
});

test("stop during signaling ignores the late answer and prevents concurrent offers", async () => {
  const s = setup(); let resolve;
  s.context.postJson = () => new Promise((done) => { resolve = done; });
  const pending = s.context.startBrowserMedia(s.button);
  while (!resolve) await Promise.resolve();
  await s.context.startBrowserMedia(s.button); assert.equal(s.peers.length, 1);
  s.context.stopBrowserMedia(); resolve({ sdp_answer: "late" }); await pending;
  assert.equal(s.state.browserMediaPeer, null); assert.equal(s.reconnect.disabled, true);
  assert.equal(s.state.browserMediaNegotiating, false);
});

test("spectator joins directly, negotiates input only after slot ownership, releases old grant", async () => {
  const s = setup(); s.button.dataset.browserMediaRole = "spectator";
  s.button.dataset.browserMediaGrant = "old-grant";
  await s.context.startBrowserMedia(s.button);
  assert.equal(s.inputs.length, 0);
  const spectatorPeer = s.peers[0];
  const actions = [];
  const session = { id: "game-1", runtime_profile: { buttons_per_player: 6 }, connection_grant: { webrtc_signaling: { offer_path: "/api/v1/sessions/game-1/webrtc/offer" } } };
  s.context.clientApi = {
    reservePlayerSlot: async (...args) => { actions.push(["reserve", ...args]); assert.equal(s.inputs.length, 0); },
    connectPlayerSlot: async (...args) => { actions.push(["connect", ...args]); assert.equal(s.inputs.length, 0); return session; },
    releaseSpectatorGrant: async (...args) => actions.push(["release", ...args]),
  };
  s.state.seatId = "edited-seat"; // Active view ownership must not follow a settings edit.
  await s.context.joinBrowserSpectator(2);
  assert.equal(s.state.browserMediaContext.role, "player");
  assert.equal(s.state.browserMediaContext.seatId, "seat-1");
  assert.equal(s.inputs.length, 1); assert.equal(spectatorPeer.connectionState, "closed");
  assert.equal(s.inputs[0].touchCount, "6");
  assert.deepEqual(actions, [["reserve", "game-1", 2, "seat-1"], ["connect", "game-1", 2, "seat-1"], ["release", "game-1", "old-grant", "seat-1"]]);
  assert.equal(s.calls.at(-1).body.role, "player");
  assert.equal(s.calls.at(-1).body.spectator_grant_id, "old-grant");
  assert.equal(s.calls.at(-1).body.seat_id, "seat-1");
});

test("failed slot reservation keeps the existing spectator peer running", async () => {
  const s = setup(); s.button.dataset.browserMediaRole = "spectator";
  await s.context.startBrowserMedia(s.button);
  const peer = s.peers[0];
  s.context.clientApi = { reservePlayerSlot: async () => { throw Error("slot taken"); } };
  await s.context.joinBrowserSpectator(2);
  assert.equal(s.state.browserMediaPeer, peer); assert.notEqual(peer.connectionState, "closed");
  assert.equal(s.inputs.length, 0); assert.equal(s.state.browserJoinPending, false);
});

test("Leave Game releases the player slot rather than keeping a disconnected reservation", async () => {
  const s = setup(); await s.context.startBrowserMedia(s.button);
  const released = [];
  s.context.clientApi = {
    releasePlayerSlot: async (...args) => { assert.equal(s.peers[0].connectionState, "closed"); released.push(args); },
    disconnectPlayerSlot: () => assert.fail("Deliberate leave must release, not disconnect"),
  };
  await s.context.leaveBrowserGame();
  assert.deepEqual(released, [["game-1", 2, "seat-1"]]);
  assert.equal(s.player.hidden, true); assert.equal(s.state.browserExitPending, false);
  assert.equal(s.state.browserMediaContext, null);
});

test("spectator Leave releases only its grant and failed player leave remains retryable", async () => {
  const s = setup(); s.button.dataset.browserMediaRole = "spectator"; s.button.dataset.browserMediaGrant = "viewer-grant";
  await s.context.startBrowserMedia(s.button);
  const released = [];
  s.context.clientApi = { releaseSpectatorGrant: async (...args) => released.push(args) };
  await s.context.leaveBrowserGame();
  assert.deepEqual(released, [["game-1", "viewer-grant", "seat-1"]]); assert.equal(s.player.hidden, true);
  await s.context.startBrowserMedia({ dataset: { ...s.button.dataset, browserMediaRole: "player", browserMediaGrant: undefined } });
  const context = s.state.browserMediaContext;
  s.context.clientApi = { releasePlayerSlot: async () => { throw Error("offline"); } };
  await s.context.leaveBrowserGame();
  assert.equal(s.state.browserMediaContext, context); assert.equal(s.player.hidden, false);
  assert.equal(s.state.browserMediaPeer, null); assert.match(s.status.textContent, /offline/);
  s.context.clientApi.releasePlayerSlot = async () => {};
  await s.context.leaveBrowserGame(); assert.equal(s.player.hidden, true);
});
