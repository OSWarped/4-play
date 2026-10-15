const assert = require("node:assert/strict");
const { test } = require("node:test");
const Input = require("../control-plane/server/static/client/input.js");
const Touch = Input.TouchControls;

function setup(touch = true, count = 2) {
  const messages = [], listeners = new Map(), windowListeners = new Map(), focusListeners = new Map();
  let hit = null, buttons = [], tick;
  const doc = { hidden: false, hasFocus: () => true, activeElement: null,
    elementFromPoint: () => hit, addEventListener() {}, removeEventListener() {} };
  const panel = { hidden: true, captures: new Set(),
    contains: (element) => buttons.includes(element), focus() { doc.activeElement = this; },
    addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: (name) => listeners.delete(name),
    setPointerCapture(id) { this.captures.add(id); }, releasePointerCapture(id) { this.captures.delete(id); },
    querySelectorAll: () => buttons,
    set innerHTML(value) {
      buttons = [...value.matchAll(/data-touch-keys="([^"]*)" data-touch-group="([^"]*)"/g)].map((match) => ({
        dataset: { touchKeys: match[1], touchGroup: match[2] }, pressed: "false",
        closest() { return this; }, setAttribute(name, value) { this.pressed = value; },
      }));
    },
  };
  const surface = { contains: (element) => element === panel || buttons.includes(element),
    addEventListener: (name, fn) => focusListeners.set(name, fn), removeEventListener: (name) => focusListeners.delete(name) };
  doc.activeElement = panel;
  const env = { document: doc, navigator: { maxTouchPoints: touch ? 5 : 0 },
    addEventListener: (name, fn) => windowListeners.set(name, fn), removeEventListener: (name) => windowListeners.delete(name),
    setInterval: (fn) => { tick = fn; return 1; }, clearInterval() {} };
  const channel = { readyState: "open", bufferedAmount: 0, send: (text) => messages.push(JSON.parse(text)) };
  const input = new Input(channel, surface, env);
  input.bindTouch(panel, count); input.start();
  const button = (codes) => buttons.find((button) => button.dataset.touchKeys === codes);
  const event = (id, target = null) => ({ pointerId: id, target, pointerType: "touch", preventDefault() {}, clientX: 1, clientY: 1 });
  const fire = (name, id, target) => listeners.get(name)(event(id, target));
  return { input, panel, doc, messages, button, fire, listeners, windowListeners, focusListeners,
    tick: () => tick(), hit: (target) => { hit = target; }, buttons: () => buttons };
}

test("capability detection and exact game action counts including six-button layout", () => {
  assert.equal(Touch.supported({ navigator: { maxTouchPoints: 0 }, matchMedia: () => ({ matches: true }) }), true);
  assert.equal(setup(false).panel.hidden, true);
  for (const count of [0, 1, 2, 5, 6]) {
    const s = setup(true, count);
    assert.equal(s.panel.hidden, false);
    assert.equal(s.buttons().filter((button) => button.dataset.touchGroup === "attack").length, count);
    assert.ok(s.button("Digit1")); assert.ok(s.button("Digit2"));
    s.input.stop();
  }
  for (const invalid of [undefined, null, "", "oops", -1, 7, 1.5]) assert.equal(Touch.buttonCount(invalid), null);
  assert.match(Touch.markup(null), /count unavailable/);
});

test("held diagonal plus two attacks survives heartbeat and releases independently", () => {
  const s = setup();
  s.fire("pointerdown", 1, s.button("KeyW KeyD"));
  s.fire("pointerdown", 2, s.button("KeyJ"));
  s.fire("pointerdown", 3, s.button("KeyK"));
  s.tick(); s.tick();
  assert.deepEqual({ ...s.messages.at(-1), sequence: 0 }, { sequence: 0, axis_x: 1, axis_y: -1, buttons: 3 });
  s.fire("pointerup", 2);
  assert.equal(s.messages.at(-1).buttons, 2); assert.equal(s.messages.at(-1).axis_x, 1);
  s.fire("pointercancel", 1);
  assert.equal(s.messages.at(-1).axis_x, 0); assert.equal(s.messages.at(-1).buttons, 2);
  s.fire("lostpointercapture", 3); assert.equal(s.messages.at(-1).buttons, 0);
});

test("sliding a thumb changes direction and leaving its control group neutralizes it", () => {
  const s = setup(); s.fire("pointerdown", 1, s.button("KeyD"));
  s.hit(s.button("KeyS KeyD")); s.fire("pointermove", 1);
  assert.equal(s.messages.at(-1).axis_y, 1);
  s.hit(s.button("Digit1")); s.fire("pointermove", 1);
  assert.equal(s.messages.at(-1).axis_x, 0); assert.equal(s.messages.at(-1).buttons, 0);
  s.hit(s.button("KeyA")); s.fire("pointermove", 1); assert.equal(s.messages.at(-1).axis_x, -1);
  s.hit(null); s.fire("pointermove", 1); assert.equal(s.messages.at(-1).axis_x, 0);
});

test("same action from multiple fingers and keyboard uses independent ownership", () => {
  const s = setup();
  s.fire("pointerdown", 1, s.button("KeyJ")); s.fire("pointerdown", 2, s.button("KeyJ"));
  s.input.key({ code: "KeyJ", preventDefault() {} }, true);
  s.fire("pointerup", 1); assert.equal(s.messages.at(-1).buttons, 1);
  s.fire("pointerup", 2); assert.equal(s.messages.at(-1).buttons, 1);
  s.input.key({ code: "KeyJ", preventDefault() {} }, false); assert.equal(s.messages.at(-1).buttons, 0);
});

test("coin/start combine and internal focus movement does not release another finger", () => {
  const s = setup(); s.fire("pointerdown", 1, s.button("Digit1")); s.fire("pointerdown", 2, s.button("Digit2"));
  s.focusListeners.get("focusout")({ relatedTarget: s.panel }); s.tick();
  assert.equal(s.messages.at(-1).buttons, 192);
  s.doc.activeElement = {}; s.focusListeners.get("focusout")({ relatedTarget: {} });
  assert.equal(s.messages.at(-1).buttons, 0); assert.equal(s.panel.captures.size, 0);
});

test("backgrounding, rotation, teardown and reconnect cannot retain or revive a touch", () => {
  const s = setup();
  for (const name of ["blur", "pagehide", "orientationchange"]) {
    s.fire("pointerdown", 1, s.button("KeyD")); s.windowListeners.get(name)(); s.tick();
    assert.equal(s.messages.at(-1).axis_x, 0); assert.equal(s.panel.captures.size, 0);
    assert.ok(s.buttons().every((button) => button.pressed === "false"));
  }
  s.fire("pointerdown", 1, s.button("KeyD")); s.doc.hidden = true; s.tick();
  assert.equal(s.messages.at(-1).axis_x, 0); s.doc.hidden = false;
  s.fire("pointerdown", 1, s.button("KeyD")); s.input.stop();
  assert.equal(s.panel.hidden, true); assert.equal(s.listeners.size, 0); assert.equal(s.windowListeners.size, 0);
  s.input.setTouch(2, ["KeyD"]); assert.equal(s.input.touchKeys.size, 0);
  s.input.start(); s.tick(); assert.equal(s.panel.hidden, false); assert.equal(s.messages.at(-1).axis_x, 0);
});
