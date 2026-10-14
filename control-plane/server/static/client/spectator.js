/* View-only menu interaction and cancellation-safe player-slot acquisition. */
(() => {
  class SpectatorOverlay {
    constructor(surface, panel, options = {}, environment = globalThis) {
      this.surface = surface;
      this.panel = panel;
      this.options = options;
      this.env = environment;
      this.running = false;
      this.visible = false;
      this.busy = false;
      this.hovered = false;
      this.hideTimer = null;
      this.padTimer = null;
      this.lastPad = "neutral";
      this.onPointer = () => this.show();
      this.onEnter = () => { this.hovered = true; this.show(); };
      this.onLeave = () => { this.hovered = false; this.scheduleHide(); };
      this.onKey = (event) => {
        if (!this.focused() || event.repeat || event.ctrlKey || event.altKey || event.metaKey) return;
        if (event.code === "Escape") return; // Shared Escape handler owns leaving.
        this.show();
        const direction = { ArrowLeft: -1, ArrowUp: -1, KeyA: -1, KeyW: -1,
          ArrowRight: 1, ArrowDown: 1, KeyD: 1, KeyS: 1 }[event.code];
        if (direction) { event.preventDefault(); this.navigate(direction); }
      };
    }

    focused() {
      const doc = this.env.document;
      return !doc.hidden && doc.hasFocus() && this.surface.contains(doc.activeElement);
    }

    start() {
      if (this.running) return;
      this.running = true;
      this.surface.addEventListener("pointermove", this.onPointer);
      this.surface.addEventListener("pointerdown", this.onPointer);
      this.panel.addEventListener("pointerenter", this.onEnter);
      this.panel.addEventListener("pointerleave", this.onLeave);
      this.panel.addEventListener("focusout", this.onLeave);
      this.env.document.addEventListener("keydown", this.onKey, true);
      this.padTimer = this.env.setInterval(() => this.pollGamepad(), 100);
      this.show();
    }

    show() {
      if (!this.running) return;
      const newlyVisible = !this.visible;
      this.visible = true;
      this.panel.inert = false;
      this.panel.setAttribute("aria-hidden", "false");
      this.panel.classList.add("is-visible");
      if (newlyVisible) this.options.onShow?.();
      this.scheduleHide();
    }

    scheduleHide() {
      if (this.hideTimer !== null) this.env.clearTimeout(this.hideTimer);
      if (!this.running) return;
      this.hideTimer = this.env.setTimeout(() => {
        this.hideTimer = null;
        if (this.busy || this.hovered || this.panel.contains(this.env.document.activeElement)) {
          this.scheduleHide();
          return;
        }
        this.hide();
      }, 3500);
    }

    hide() {
      this.visible = false;
      this.panel.classList.remove("is-visible");
      this.panel.setAttribute("aria-hidden", "true");
      this.panel.inert = true;
    }

    actions() { return Array.from(this.panel.querySelectorAll("button:not(:disabled)")); }

    navigate(direction) {
      if (this.busy) return;
      const actions = this.actions();
      if (!actions.length) return;
      const index = actions.indexOf(this.env.document.activeElement);
      actions[(index < 0 ? 0 : (index + direction + actions.length) % actions.length)].focus();
    }

    pollGamepad() {
      if (!this.running) return;
      let action = "neutral";
      try {
        const pad = Array.from(this.env.navigator.getGamepads?.() || []).find((pad) => pad?.mapping === "standard");
        if (pad) {
          if (pad.buttons[14]?.pressed || pad.buttons[12]?.pressed || pad.axes[0] < -0.35 || pad.axes[1] < -0.35) action = "previous";
          else if (pad.buttons[15]?.pressed || pad.buttons[13]?.pressed || pad.axes[0] > 0.35 || pad.axes[1] > 0.35) action = "next";
          else if (pad.buttons[0]?.pressed) action = "confirm";
          else if (pad.buttons.some((button) => button?.pressed)) action = "wake";
        }
      } catch { /* Keyboard and mouse remain usable without Gamepad API access. */ }
      const previous = this.lastPad;
      this.lastPad = action;
      if (!this.focused() || action === "neutral" || action === previous) return;
      const wasVisible = this.visible;
      this.show();
      if (action === "previous" || action === "next") this.navigate(action === "previous" ? -1 : 1);
      // Waking the menu must never accidentally claim a player slot.
      if (action === "confirm" && wasVisible && previous === "neutral" && !this.busy) {
        const active = this.env.document.activeElement;
        if (this.actions().includes(active)) active.click();
      }
    }

    setBusy(busy) { this.busy = busy; if (busy) this.show(); }

    stop() {
      this.running = false;
      if (this.hideTimer !== null) this.env.clearTimeout(this.hideTimer);
      if (this.padTimer !== null) this.env.clearInterval(this.padTimer);
      this.hideTimer = this.padTimer = null;
      this.surface.removeEventListener("pointermove", this.onPointer);
      this.surface.removeEventListener("pointerdown", this.onPointer);
      this.panel.removeEventListener("pointerenter", this.onEnter);
      this.panel.removeEventListener("pointerleave", this.onLeave);
      this.panel.removeEventListener("focusout", this.onLeave);
      this.env.document.removeEventListener("keydown", this.onKey, true);
      this.hide();
    }
  }

  function joinableSlots(session, seatId) {
    return (session?.player_slots || []).filter((slot) => slot.state === "open"
      || (["reserved", "disconnected"].includes(slot.state) && slot.seat_id === seatId));
  }

  async function acquirePlayer(api, context, player, isCurrent) {
    if (context?.role !== "spectator" || !Number.isInteger(player) || player <= 0) {
      throw Error("Select a valid player slot while spectating.");
    }
    let reserved = false;
    try {
      if (!isCurrent()) return null;
      await api.reservePlayerSlot(context.sessionId, player, context.seatId);
      reserved = true;
      if (!isCurrent()) return null;
      const session = await api.connectPlayerSlot(context.sessionId, player, context.seatId);
      if (!isCurrent()) return null;
      reserved = false; // Ownership handed to the caller; it must offer Leave/Retry.
      return session;
    } finally {
      if (reserved) {
        // The backend checks seat ownership; cancellation never releases another seat.
        try { await api.disconnectPlayerSlot(context.sessionId, player, context.seatId); }
        catch (error) { throw Error(`Pending player-slot cleanup failed; use the session Leave action. ${error.message}`); }
      }
    }
  }

  const exports = { SpectatorOverlay, joinableSlots, acquirePlayer };
  if (typeof module !== "undefined" && module.exports) module.exports = exports;
  else globalThis.FourPlaySpectator = exports;
})();
