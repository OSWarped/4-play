const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");

function login(fetch) {
  let submit;
  const input = { value: "a".repeat(64), focus() {} };
  const button = {};
  const status = {};
  const form = { reportValidity: () => true, addEventListener: (_, callback) => { submit = callback; } };
  const elements = { "tester-form": form, invitation: input, "tester-submit": button, "tester-status": status };
  const navigation = [];
  vm.runInNewContext(fs.readFileSync("control-plane/server/static/client-v2/tester-access.js", "utf8"), {
    document: { getElementById: (id) => elements[id] }, fetch,
    location: { replace: (path) => navigation.push(path) },
  });
  return { input, button, status, navigation, submit: () => submit({ preventDefault() {} }) };
}

test("invitation travels only in POST; form cleared; navigation has no credentials", async () => {
  let request;
  const state = login(async (url, options) => { request = { url, options }; return { ok: true, json: async () => ({ ok: true }) }; });
  await state.submit();
  assert.equal(request.url, "/client-v2/tester-access");
  assert.equal(request.options.method, "POST");
  assert.equal(request.options.credentials, "same-origin");
  assert.equal(JSON.parse(request.options.body).invitation, "a".repeat(64));
  assert.equal(state.input.value, "");
  assert.deepEqual(state.navigation, ["/client-v4"]);
});

test("rejections remain in the form and allow retry without leaking the code", async () => {
  const state = login(async () => ({ ok: false, json: async () => ({ message: "Invitation expired." }) }));
  await state.submit();
  assert.equal(state.status.textContent, "Invitation expired.");
  assert.equal(state.button.disabled, false);
  assert.equal(state.input.value, "");
  assert.equal(state.navigation.length, 0);
});

test("duplicate form submissions cannot spend two enrollments", async () => {
  let release;
  let calls = 0;
  const state = login(() => { calls++; return new Promise((resolve) => { release = resolve; }); });
  const first = state.submit();
  await state.submit();
  assert.equal(calls, 1);
  release({ ok: true, json: async () => ({ ok: true }) });
  await first;
});

test("V2 adapter redirects only scoped tester admission errors", async () => {
  const navigation = [];
  const root = { location: { origin: "https://arcade.example.test", replace: (s) => navigation.push(s) }, URL, AbortController,
    document: { body: { dataset: { clientVersion: "2" }, append() {}, classList: { toggle() {} } }, createElement: () => ({ setAttribute() {} }) },
    fetch: async () => ({ ok: false, status: 401, json: async () => ({ code: "tester_access_required", message: "Invitation required" }) }) };
  vm.runInNewContext(fs.readFileSync("control-plane/server/static/client-v2/experience.js", "utf8"), { window: root });
  await assert.rejects(root.FourPlayV2.bootstrap(), /Invitation required/);
  assert.deepEqual(navigation, ["/client-v2/tester-access"]);
  navigation.length = 0;
  root.fetch = async () => ({ ok: false, status: 403, json: async () => ({ code: "tester_edge_denied" }) });
  await assert.rejects(root.FourPlayV2.bootstrap());
  assert.equal(navigation.length, 0);
});
