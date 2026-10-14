/* Exercise the shipped client lifecycle with deterministic browser/API fakes. */
const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs");
const vm = require("node:vm");
const Spectator = require("../control-plane/server/static/client/spectator.js");

function setup() {
  const peers = [], inputs = [], monitors = [], calls = [];
  const state = { seatId: "seat-1", browserExitPending: false, browserMediaNegotiating: false, sessions: [] };
  const video = { focus() {}, pause() {}, srcObject: null, play: async () => {} };
  const player = { hidden: true, scrollIntoView() {} };
  const status = { textContent: "" }, reconnect = {};
  const inputStatus = { textContent: "" };
  const slots = { contains: () => false, replaceChildren() {}, querySelectorAll: () => [] };
  const elements = { "#spectator-player-slots": slots, "#spectator-actions": {}, "#spectator-action-status": {} };
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
    MediaStream: class { constructor() { this.tracks = []; } addTrack(track) { this.tracks.push(track); } getTracks() { return this.tracks; } },
    browserPlayer: player, browserMediaVideo: video, browserMediaState: status,
    reconnectBrowserMediaButton: reconnect, browserReceiverStats: { textContent: "" },
    document: { querySelector: (selector) => elements[selector] || inputStatus },
    window: { RTCPeerConnection: Peer, setInterval: () => 1, clearInterval() {}, location: { origin: "http://server" } },
    FourPlaySpectator: { ...Spectator, SpectatorOverlay: class { start() {} stop() {} setBusy() {} } },
    clientApi: {}, rememberRuntime() {}, publicPlayerHandoff: () => ({ mode: "player" }), refresh: async () => {},
    syncSettingsFromInputs() {},
    postJson: async (url, body) => { calls.push({ url, body }); return { sdp_answer: "answer" }; },
  });
  const code = fs.readFileSync("control-plane/server/static/client/app.js", "utf8");
  vm.runInContext(code.slice(code.indexOf("async function startBrowserMedia("), code.indexOf("function renderLastRuntimeHint(")), context);
  const button = { dataset: { browserMediaUrl: "http://server/api/v1/sessions/game-1/webrtc/offer",
    browserMediaRole: "player", browserMediaPlayer: "2" } };
  return { context, state, peers, inputs, monitors, calls, video, status, reconnect, button };
}

test("disconnect releases controls, reconnect preserves slot and closes old media", async () => {
  const s = setup(); await s.context.startBrowserMedia(s.button);
  const peer = s.peers[0]; peer.connectionState = "connected"; peer.listeners.connectionstatechange();
  peer.channel.listeners.open(); assert.equal(s.inputs[0].running, true);
  peer.connectionState = "disconnected"; peer.listeners.connectionstatechange();
  assert.equal(s.inputs[0].running, false);
  const retry = s.state.browserMediaRetry;
  await s.context.startBrowserMedia({ dataset: retry });
  assert.equal(peer.connectionState, "closed"); assert.equal(s.monitors[0].stopped, true);
  assert.equal(s.state.browserMediaContext.playerNumber, 2);
  assert.equal(s.calls.length, 2); // Only signaling; no disconnect/re-reserve request.
  const before = s.status.textContent;
  peer.listeners.track({ track: { kind: "video" } }); peer.listeners.connectionstatechange();
  assert.equal(s.video.srcObject.getTracks().length, 0);
  assert.equal(s.status.textContent, before);
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
  const session = { id: "game-1", connection_grant: { webrtc_signaling: { offer_path: "/api/v1/sessions/game-1/webrtc/offer" } } };
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
