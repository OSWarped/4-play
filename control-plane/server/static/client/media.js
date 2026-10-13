/* Receiver diagnostics only: never buffer media or expose private grants. */
(() => {
  class BrowserMediaMonitor {
    constructor(peer, report, environment = globalThis) {
      this.peer = peer;
      this.report = report;
      this.env = environment;
      this.timer = null;
      this.stopped = false;
      this.busy = false;
      this.previous = new Map();
      this.lastProgress = null;
      this.startedAt = 0;
    }

    start() {
      if (this.timer !== null || this.stopped) return;
      this.startedAt = this.env.performance.now();
      this.timer = this.env.setInterval(() => this.sample(), 1000);
      this.sample();
    }

    async sample() {
      if (this.stopped || this.busy) return;
      this.busy = true;
      try {
        const stats = await this.peer.getStats();
        if (this.stopped) return;
        const now = this.env.performance.now();
        let frames = 0, dropped = 0, lost = 0, jitter = 0, fps = 0;
        let video = false, progressed = false, rtt = null, decodeCounter = false;
        const next = new Map();
        stats.forEach((entry) => {
          if (entry.type === "candidate-pair" && entry.state === "succeeded" && entry.nominated) {
            rtt = entry.currentRoundTripTime ?? rtt;
          }
          if (entry.type !== "inbound-rtp" || (entry.kind || entry.mediaType) !== "video") return;
          video = true;
          const count = entry.framesDecoded ?? 0;
          const progress = entry.framesDecoded ?? entry.bytesReceived ?? 0;
          decodeCounter ||= typeof entry.framesDecoded === "number";
          frames += count;
          dropped += entry.framesDropped ?? 0;
          lost += entry.packetsLost ?? 0;
          jitter = Math.max(jitter, entry.jitter ?? 0);
          const old = this.previous.get(entry.id);
          progressed ||= progress > (old?.progress ?? 0);
          if (old && entry.timestamp > old.timestamp && count >= old.frames) {
            fps += (count - old.frames) * 1000 / (entry.timestamp - old.timestamp);
          }
          next.set(entry.id, { frames: count, progress, timestamp: entry.timestamp });
        });
        this.previous = next;
        if (progressed) this.lastProgress = now;
        const connected = this.peer.connectionState === "connected";
        const stalled = connected && now - (this.lastProgress ?? this.startedAt) >= 3000;
        this.report({ video, frames, dropped, lost, jitterMs: jitter * 1000,
          rttMs: rtt === null ? null : rtt * 1000, fps, stalled, decodeCounter });
      } catch {
        if (!this.stopped) this.report(null); // Some browsers omit receiver stats.
      } finally {
        this.busy = false;
      }
    }

    stop() {
      this.stopped = true;
      if (this.timer !== null) this.env.clearInterval(this.timer);
      this.timer = null;
      this.previous.clear();
    }
  }
  if (typeof module !== "undefined" && module.exports) module.exports = BrowserMediaMonitor;
  else globalThis.FourPlayBrowserMediaMonitor = BrowserMediaMonitor;
})();
