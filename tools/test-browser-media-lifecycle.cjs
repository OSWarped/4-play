/* Exercise the shipped client lifecycle with deterministic browser/API fakes. */
const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs");
const vm = require("node:vm");

function setup() {
  const peers = [], inputs = [], monitors = [], calls = [];
  const state = { seatId: "seat-1", browserExitPending: false, browserMediaNegotiating: false };
  const video = { focus() {}, pause() {}, srcObject: null, play: async () => {} };
  const player = { hidden: true, scrollIntoView() {} };
  const status = { textContent: "" }, reconnect = {};
  const inputStatus = { textContent: "" };
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
    document: { querySelector: () => inputStatus }, window: { RTCPeerConnection: Peer, setInterval: () => 1, clearInterval() {} },
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
