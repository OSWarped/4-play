/* Complete controller states over an unreliable WebRTC channel. */
(() => {
  const buttons = new Map([
    ["KeyJ", 1], ["KeyK", 2], ["KeyL", 4], ["KeyM", 8],
    ["Comma", 16], ["Period", 32], ["Digit1", 64], ["Digit2", 128],
  ]);
  const directions = new Set(["KeyW", "KeyA", "KeyS", "KeyD"]);

  class BrowserInput {
    constructor(channel, surface, environment = globalThis) {
      this.channel = channel;
      this.surface = surface;
      this.env = environment;
      this.keys = new Set();
      this.sequence = 0;
      this.timer = null;
      this.onKeyDown = (event) => this.key(event, true);
      this.onKeyUp = (event) => this.key(event, false);
      this.onBlur = () => this.release();
      this.onVisibility = () => { if (this.env.document.hidden) this.release(); };
    }

    focused() {
      const doc = this.env.document;
      const active = doc.activeElement;
      return !doc.hidden && doc.hasFocus() && this.surface.contains(active)
        && !active?.matches?.("input, textarea, select, [contenteditable=true]");
    }

    start() {
      if (this.timer !== null) return;
      const doc = this.env.document;
      doc.addEventListener("keydown", this.onKeyDown, true);
      doc.addEventListener("keyup", this.onKeyUp, true);
      doc.addEventListener("visibilitychange", this.onVisibility);
      this.env.addEventListener("blur", this.onBlur);
      this.surface.addEventListener("focusout", this.onBlur);
      this.timer = this.env.setInterval(() => this.send(), 20);
      this.send();
    }

    key(event, pressed) {
      if (!buttons.has(event.code) && !directions.has(event.code)) return;
      if (pressed && (!this.focused() || event.ctrlKey || event.altKey || event.metaKey)) return;
      if (!pressed && !this.keys.has(event.code)) return;
      event.preventDefault();
      if (pressed) this.keys.add(event.code);
      else this.keys.delete(event.code);
      this.send();
    }

    snapshot() {
      if (!this.focused()) {
        this.keys.clear();
        return { buttons: 0, axis_x: 0, axis_y: 0 };
      }
      let mask = 0;
      for (const code of this.keys) mask |= buttons.get(code) || 0;
      let x = Number(this.keys.has("KeyD")) - Number(this.keys.has("KeyA"));
      let y = Number(this.keys.has("KeyS")) - Number(this.keys.has("KeyW"));
      try {
        const pad = Array.from(this.env.navigator.getGamepads?.() || []).find((pad) => pad?.mapping === "standard");
        if (pad) {
          [0, 1, 2, 3, 4, 5, 8, 9].forEach((index, bit) => {
            if (pad.buttons[index]?.pressed) mask |= 1 << bit;
          });
          const axis = (value) => value < -0.35 ? -1 : value > 0.35 ? 1 : 0;
          x ||= Number(Boolean(pad.buttons[15]?.pressed)) - Number(Boolean(pad.buttons[14]?.pressed)) || axis(pad.axes[0] || 0);
          y ||= Number(Boolean(pad.buttons[13]?.pressed)) - Number(Boolean(pad.buttons[12]?.pressed)) || axis(pad.axes[1] || 0);
        }
      } catch { /* Keyboard remains usable when browser gamepad access is unavailable. */ }
      return { buttons: mask, axis_x: x, axis_y: y };
    }

    send(neutral = false) {
      if (this.channel.readyState !== "open" || this.channel.bufferedAmount > 4096) return;
      this.sequence = (this.sequence + 1) >>> 0;
      const state = neutral ? { buttons: 0, axis_x: 0, axis_y: 0 } : this.snapshot();
      try { this.channel.send(JSON.stringify({ sequence: this.sequence, ...state })); }
      catch { /* Channel closure is also handled by the runtime input timeout. */ }
    }

    release() {
      this.keys.clear();
      this.send(true);
    }

    stop() {
      this.release();
      if (this.timer !== null) this.env.clearInterval(this.timer);
      this.timer = null;
      const doc = this.env.document;
      doc.removeEventListener("keydown", this.onKeyDown, true);
      doc.removeEventListener("keyup", this.onKeyUp, true);
      doc.removeEventListener("visibilitychange", this.onVisibility);
      this.env.removeEventListener("blur", this.onBlur);
      this.surface.removeEventListener("focusout", this.onBlur);
    }
  }

  if (typeof module !== "undefined" && module.exports) module.exports = BrowserInput;
  else globalThis.FourPlayBrowserInput = BrowserInput;
})();
