/* Complete controller states over an unreliable WebRTC channel. */
(() => {
  const buttons = new Map([
    ["KeyJ", 1], ["KeyK", 2], ["KeyL", 4], ["KeyM", 8],
    ["Comma", 16], ["Period", 32], ["Digit1", 64], ["Digit2", 128],
  ]);
  const directions = new Set(["KeyW", "KeyA", "KeyS", "KeyD"]);

  class BrowserInput {
    static bindEscape(surface, leave, release, environment = globalThis) {
      const doc = environment.document;
      const keydown = (event) => {
        if (event.code !== "Escape" || event.repeat || doc.hidden || !doc.hasFocus()
            || !surface.contains(doc.activeElement)) return;
        release();
        // Browsers reserve Escape for leaving fullscreen. A second press in
        // windowed mode leaves the game; do not depend on cancelling native Esc.
        if (doc.fullscreenElement) return;
        event.preventDefault();
        leave();
      };
      const fullscreen = () => { if (!doc.fullscreenElement) release(); };
      doc.addEventListener("keydown", keydown, true);
      doc.addEventListener("fullscreenchange", fullscreen);
      return () => {
        doc.removeEventListener("keydown", keydown, true);
        doc.removeEventListener("fullscreenchange", fullscreen);
      };
    }

    constructor(channel, surface, environment = globalThis) {
      this.channel = channel;
      this.surface = surface;
      this.env = environment;
      this.keys = new Set();
      this.touchKeys = new Map();
      this.touch = null;
      this.sequence = 0;
      this.timer = null;
      this.onKeyDown = (event) => this.key(event, true);
      this.onKeyUp = (event) => this.key(event, false);
      this.onBlur = () => this.release();
      this.onFocusOut = (event) => { if (!this.surface.contains(event.relatedTarget)) this.release(); };
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
      this.env.addEventListener("pagehide", this.onBlur);
      this.env.addEventListener("orientationchange", this.onBlur);
      this.surface.addEventListener("focusout", this.onFocusOut);
      this.timer = this.env.setInterval(() => this.send(), 20);
      this.touch?.start();
      this.send();
    }

    bindTouch(panel, count) {
      this.touch?.stop();
      this.touch = panel ? new TouchControls(this, panel, count) : null;
      if (this.timer !== null) this.touch?.start();
    }

    setTouch(pointerId, codes) {
      if (this.timer === null) return;
      const safe = codes.filter((code) => buttons.has(code) || directions.has(code));
      if (safe.length) this.touchKeys.set(pointerId, safe);
      else this.touchKeys.delete(pointerId);
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
        this.touchKeys.clear();
        this.touch?.reset();
        return { buttons: 0, axis_x: 0, axis_y: 0 };
      }
      const keys = new Set([...this.keys, ...Array.from(this.touchKeys.values()).flat()]);
      let mask = 0;
      for (const code of keys) mask |= buttons.get(code) || 0;
      let x = Number(keys.has("KeyD")) - Number(keys.has("KeyA"));
      let y = Number(keys.has("KeyS")) - Number(keys.has("KeyW"));
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
      this.touchKeys.clear();
      this.touch?.reset();
      this.send(true);
    }

    stop() {
      this.release();
      if (this.timer !== null) this.env.clearInterval(this.timer);
      this.timer = null;
      this.touch?.stop();
      const doc = this.env.document;
      doc.removeEventListener("keydown", this.onKeyDown, true);
      doc.removeEventListener("keyup", this.onKeyUp, true);
      doc.removeEventListener("visibilitychange", this.onVisibility);
      this.env.removeEventListener("blur", this.onBlur);
      this.env.removeEventListener("pagehide", this.onBlur);
      this.env.removeEventListener("orientationchange", this.onBlur);
      this.surface.removeEventListener("focusout", this.onFocusOut);
    }
  }

  // Capability detection also covers tablets and touch-enabled laptops. No UA
  // sniffing or secure-context-only gamepad API is needed for touch input.
  class TouchControls {
    static supported(env) {
      return Boolean(env.navigator.maxTouchPoints > 0 || env.matchMedia?.("(any-pointer: coarse)").matches);
    }

    static buttonCount(value) {
      if (value === null || value === undefined || value === "") return null;
      const count = Number(value);
      return Number.isInteger(count) && count >= 0 && count <= 6 ? count : null;
    }

    static markup(count) {
      const button = (label, codes, group, title = label) => `<button type="button" data-touch-keys="${codes}" data-touch-group="${group}" aria-label="${title}" aria-pressed="false">${label}</button>`;
      const cells = [
        ["↖", "KeyW KeyA", "Up left"], ["↑", "KeyW", "Up"], ["↗", "KeyW KeyD", "Up right"],
        ["←", "KeyA", "Left"], ["●", "", "Neutral"], ["→", "KeyD", "Right"],
        ["↙", "KeyS KeyA", "Down left"], ["↓", "KeyS", "Down"], ["↘", "KeyS KeyD", "Down right"],
      ];
      const actions = ["KeyJ", "KeyK", "KeyL", "KeyM", "Comma", "Period"];
      return `<div class="touch-dpad" role="group" aria-label="Directional pad">${cells.map(([label, codes, title]) => button(label, codes, "direction", title)).join("")}</div>
        <div class="touch-right"><div class="touch-system">${button("Coin", "Digit1", "system")}${button("Start", "Digit2", "system")}</div>
        <div class="touch-attacks" role="group" aria-label="Action buttons">${actions.slice(0, count ?? 0).map((code, index) => button(String(index + 1), code, "attack", `Action ${index + 1}`)).join("")}</div>
        ${count === null ? '<small>Action-button count unavailable. Refresh and rejoin.</small>' : ""}</div>`;
    }

    constructor(input, panel, count) {
      this.input = input;
      this.panel = panel;
      this.count = TouchControls.buttonCount(count);
      this.active = new Map();
      this.running = false;
      this.handlers = {
        pointerdown: (event) => this.down(event),
        pointermove: (event) => this.move(event),
        pointerup: (event) => this.up(event),
        pointercancel: (event) => this.up(event),
        lostpointercapture: (event) => this.up(event),
        contextmenu: (event) => event.preventDefault(),
      };
    }

    start() {
      if (this.running || !TouchControls.supported(this.input.env)) return;
      this.running = true;
      this.panel.innerHTML = TouchControls.markup(this.count);
      this.panel.hidden = false;
      for (const [name, handler] of Object.entries(this.handlers)) this.panel.addEventListener(name, handler);
    }

    target(element) {
      const button = element?.closest?.("[data-touch-keys]");
      return button && this.panel.contains(button) ? button : null;
    }

    down(event) {
      const button = this.target(event.target);
      if (!this.running || !button || (event.pointerType === "mouse" && event.button !== 0)) return;
      event.preventDefault();
      this.panel.focus({ preventScroll: true });
      try { this.panel.setPointerCapture(event.pointerId); } catch { return; }
      this.active.set(event.pointerId, { button, group: button.dataset.touchGroup });
      this.input.setTouch(event.pointerId, button.dataset.touchKeys.split(" "));
      this.paint();
    }

    move(event) {
      const held = this.active.get(event.pointerId);
      if (!held) return;
      event.preventDefault();
      const hit = this.target(this.input.env.document.elementFromPoint(event.clientX, event.clientY));
      // A thumb can slide through diagonals or action buttons without lifting,
      // but cannot accidentally become Coin/Start or cross to the other hand.
      held.button = hit?.dataset.touchGroup === held.group ? hit : null;
      this.input.setTouch(event.pointerId, held.button?.dataset.touchKeys.split(" ") || []);
      this.paint();
    }

    up(event) {
      if (!this.active.delete(event.pointerId)) return;
      this.input.setTouch(event.pointerId, []);
      try { this.panel.releasePointerCapture(event.pointerId); } catch { /* Already cancelled. */ }
      this.paint();
    }

    paint() {
      const held = new Set(Array.from(this.active.values(), (value) => value.button));
      this.panel.querySelectorAll("[data-touch-keys]").forEach((button) => button.setAttribute("aria-pressed", String(held.has(button))));
    }

    reset() {
      const pointers = [...this.active.keys()];
      this.active.clear();
      for (const id of pointers) { try { this.panel.releasePointerCapture(id); } catch { /* No capture. */ } }
      this.paint();
    }

    stop() {
      this.reset();
      this.running = false;
      this.panel.hidden = true;
      for (const [name, handler] of Object.entries(this.handlers)) this.panel.removeEventListener(name, handler);
    }
  }

  BrowserInput.TouchControls = TouchControls;

  if (typeof module !== "undefined" && module.exports) module.exports = BrowserInput;
  else globalThis.FourPlayBrowserInput = BrowserInput;
})();
