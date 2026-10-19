const assert = require("node:assert/strict");
const { test } = require("node:test");
const Monitor = require("../control-plane/server/static/client/media.js");

function renewalSetup(delay, renew = () => {}) {
  let now = 0, tick, visibility, intervals = 0, cleared = 0;
  const env = {
    performance: { now: () => now },
    setInterval(callback) { intervals++; tick = callback; return 7; },
    clearInterval(id) { assert.equal(id, 7); cleared++; },
    document: { addEventListener(name, callback) { assert.equal(name, "visibilitychange"); visibility = callback; },
      removeEventListener(name, callback) { assert.equal(name, "visibilitychange"); if (visibility === callback) visibility = null; } },
  };
  const timer = new Monitor.CredentialRenewal(delay, renew, env);
  return { timer, env, set: (time) => { now = time; }, tick: () => tick(),
    visible: () => visibility?.(), counts: () => ({ intervals, cleared }) };
}

test("WAN credential refresh uses monotonic elapsed time and runs only once", () => {
  let renewals = 0;
  const s = renewalSetup(2700000, () => { renewals++; });
  s.timer.start(); s.timer.start();
  s.set(2699999); s.tick(); assert.equal(renewals, 0);
  s.set(2700000); s.tick(); s.tick();
  assert.equal(renewals, 1); assert.deepEqual(s.counts(), { intervals: 1, cleared: 1 });
});

test("LAN or invalid renewal metadata never creates a timer", () => {
  for (const value of [undefined, null, 0, -1, 999, 3600001, "2700000", NaN, Infinity]) {
    const s = renewalSetup(value); s.timer.start();
    assert.deepEqual(s.counts(), { intervals: 0, cleared: 0 });
  }
});

test("stopped credential refresh cannot reconnect a departed session", () => {
  let renewals = 0;
  const s = renewalSetup(1000, () => { renewals++; });
  s.timer.start(); const tick = s.tick;
  s.timer.stop(); s.timer.stop(); s.set(2000); tick(); s.visible();
  assert.equal(renewals, 0); assert.deepEqual(s.counts(), { intervals: 1, cleared: 1 });
});

test("return from a throttled background tab checks overdue renewal", () => {
  let renewals = 0;
  const s = renewalSetup(1000, () => { renewals++; });
  s.timer.start(); s.set(100000); s.visible(); s.tick();
  assert.equal(renewals, 1);
});

test("a busy promotion delays scheduling without making repeated network attempts", () => {
  let busy = true, attempts = 0;
  const s = renewalSetup(1000, () => { if (busy) return false; attempts++; });
  s.timer.start(); s.set(1000); s.tick(); s.tick(); assert.equal(attempts, 0);
  busy = false; s.tick(); s.tick(); assert.equal(attempts, 1);
});

function setup() {
  let now = 0, stats = [], tick, cleared = 0;
  const reports = [];
  const peer = { connectionState: "connected", getStats: async () => stats };
  const monitor = new Monitor(peer, (report) => reports.push(report), {
    performance: { now: () => now },
    setInterval: (callback) => { tick = callback; return 7; },
    clearInterval: (id) => { assert.equal(id, 7); cleared++; },
  });
  return { peer, monitor, reports, set: (time, values) => { now = time; stats = values; },
    tick: () => tick(), cleared: () => cleared };
}
function video(frames, timestamp, extra = {}) {
  return { id: "video", type: "inbound-rtp", kind: "video", framesDecoded: frames,
    timestamp, framesDropped: 2, packetsLost: 3, jitter: 0.004, ...extra };
}

test("stats show frame rate, loss, jitter and nominated RTT without changing media", async () => {
  const s = setup();
  s.set(0, [video(60, 1000)]); s.monitor.start(); await Promise.resolve();
  s.set(1000, [video(120, 2000), { type: "candidate-pair", nominated: true,
    state: "succeeded", currentRoundTripTime: 0.012 }]);
  await s.tick();
  assert.deepEqual(s.reports.at(-1), { video: true, frames: 120, dropped: 2,
    lost: 3, jitterMs: 4, rttMs: 12, fps: 60, stalled: false, decodeCounter: true });
  s.monitor.stop(); assert.equal(s.cleared(), 1);
});

test("no frames and frozen decoder are detected, recovery clears warning", async () => {
  const s = setup(); s.monitor.start(); await Promise.resolve();
  s.set(3000, []); await s.tick(); assert.equal(s.reports.at(-1).stalled, true);
  s.set(4000, [video(10, 4000)]); await s.tick(); assert.equal(s.reports.at(-1).stalled, false);
  s.set(7000, [video(10, 7000)]); await s.tick(); assert.equal(s.reports.at(-1).stalled, true);
  s.set(8000, [video(20, 8000)]); await s.tick(); assert.equal(s.reports.at(-1).stalled, false);
  s.monitor.stop();
});

test("overlapping requests and late stats after teardown cannot update the UI", async () => {
  const s = setup(); let resolve, requests = 0;
  s.peer.getStats = () => { requests++; return new Promise((done) => { resolve = done; }); };
  s.monitor.start(); await s.monitor.sample(); assert.equal(requests, 1);
  s.monitor.stop(); resolve([video(60, 1000)]); await Promise.resolve();
  assert.equal(s.reports.length, 0); assert.equal(s.cleared(), 1);
  await s.monitor.sample(); assert.equal(requests, 1);
});

test("missing browser stats and counter reset do not produce negative frame rates", async () => {
  const s = setup(); s.set(0, [video(60, 1000)]); s.monitor.start(); await Promise.resolve();
  s.set(1000, [video(1, 2000)]); await s.tick(); assert.equal(s.reports.at(-1).fps, 0);
  s.peer.getStats = async () => { throw Error("unsupported"); };
  await s.tick(); assert.equal(s.reports.at(-1), null);
  s.monitor.stop();
});
