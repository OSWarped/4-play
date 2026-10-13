const assert = require("node:assert/strict");
const { test } = require("node:test");
const Monitor = require("../control-plane/server/static/client/media.js");

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
