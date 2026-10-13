const assert = require("node:assert/strict");
const { test } = require("node:test");
const BrowserInput = require("../control-plane/server/static/client/input.js");

function setup() {
  const messages = [];
  const active = { matches: () => false };
  const listeners = new Map();
  const document = {
    activeElement: active, hidden: false, hasFocus: () => true,
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name) => listeners.delete(name),
  };
  let heartbeat;
  const env = {
    document, navigator: {}, addEventListener() {}, removeEventListener() {},
    setInterval: (fn) => { heartbeat = fn; return 1; }, clearInterval() {},
  };
  const surface = { contains: (element) => element === active, addEventListener() {}, removeEventListener() {} };
  const channel = { readyState: "open", bufferedAmount: 0, send: (text) => messages.push(JSON.parse(text)) };
  const input = new BrowserInput(channel, surface, env);
  const key = (code, pressed) => input.key({ code, preventDefault() {} }, pressed);
  input.start();
  return { input, key, messages, document, env, channel, tick: () => heartbeat() };
}

test("held direction survives heartbeat, diagonals and attacks combine", () => {
  const s = setup();
  s.key("KeyD", true);
  s.tick(); s.tick();
  assert.ok(s.messages.slice(-3).every((state) => state.axis_x === 1));
  s.key("KeyW", true); s.key("KeyJ", true); s.key("KeyK", true);
  assert.deepEqual({ ...s.messages.at(-1), sequence: 0 }, { sequence: 0, axis_x: 1, axis_y: -1, buttons: 3 });
  s.key("KeyD", false);
  assert.equal(s.messages.at(-1).axis_x, 0);
  assert.equal(s.messages.at(-1).buttons, 3);
});

test("focus loss, hidden page, and shutdown release all buttons", () => {
  const s = setup();
  s.key("KeyJ", true); s.key("KeyD", true);
  s.document.activeElement = {};
  s.tick();
  assert.equal(s.messages.at(-1).buttons, 0);
  assert.equal(s.messages.at(-1).axis_x, 0);
  s.document.hidden = true;
  s.key("KeyK", true); s.tick();
  assert.equal(s.messages.at(-1).buttons, 0);
  s.input.stop();
  assert.equal(s.messages.at(-1).buttons, 0);
});

test("coin/start, standard gamepad combo, and disconnect", () => {
  const s = setup();
  s.key("Digit1", true); s.key("Digit2", true);
  assert.equal(s.messages.at(-1).buttons, 192);
  s.input.release();
  s.env.navigator.getGamepads = () => [{ mapping: "standard", axes: [1, -1], buttons: [{ pressed: true }, { pressed: true }] }];
  s.tick();
  assert.equal(s.messages.at(-1).buttons, 3);
  assert.equal(s.messages.at(-1).axis_y, -1);
  const before = s.messages.length;
  s.channel.readyState = "closed";
  s.tick();
  assert.equal(s.messages.length, before);
});

test("Escape releases controls and leaves a focused windowed game once", () => {
  const s = setup();
  const listeners = new Map();
  s.document.addEventListener = (name, fn) => listeners.set(name, fn);
  s.document.removeEventListener = (name) => listeners.delete(name);
  let left = 0;
  const unbind = BrowserInput.bindEscape(s.input.surface, () => left++, () => s.input.release(), s.env);
  s.key("KeyD", true);
  listeners.get("keydown")({ code: "Escape", preventDefault() {} });
  assert.equal(left, 1);
  assert.equal(s.messages.at(-1).axis_x, 0);
  listeners.get("keydown")({ code: "Escape", repeat: true, preventDefault() {} });
  assert.equal(left, 1);
  s.document.activeElement = {};
  listeners.get("keydown")({ code: "Escape", preventDefault() {} });
  assert.equal(left, 1);
  unbind();
  assert.equal(listeners.size, 0);
});

test("fullscreen Escape releases input; second Escape leaves the game", () => {
  const s = setup();
  const listeners = new Map();
  s.document.addEventListener = (name, fn) => listeners.set(name, fn);
  let left = 0;
  BrowserInput.bindEscape(s.input.surface, () => left++, () => s.input.release(), s.env);
  s.document.fullscreenElement = s.input.surface;
  s.key("KeyD", true);
  listeners.get("keydown")({ code: "Escape", preventDefault() { throw Error("Must allow native fullscreen exit"); } });
  assert.equal(left, 0);
  assert.equal(s.messages.at(-1).axis_x, 0);
  s.document.fullscreenElement = null;
  listeners.get("fullscreenchange")();
  listeners.get("keydown")({ code: "Escape", preventDefault() {} });
  assert.equal(left, 1);
});
