const { test } = require('node:test');
const assert = require('node:assert/strict');
const Provider = require('../control-plane/server/static/admin/metadata-provider.js');

test('title suggestions remove MAME revision suffixes without changing the catalog', () => {
  assert.equal(Provider.searchTitle('Aliens (World set 1)'), 'Aliens');
  assert.equal(Provider.searchTitle('Killer Instinct'), 'Killer Instinct');
});
test('artwork roles map to sensible fields, not technical metadata', () => {
  assert.equal(Provider.defaultImageField('screenshot'), 'screenshot_path');
  assert.equal(Provider.defaultImageField('clearlogo'), 'logo_path');
  assert.equal(Provider.defaultImageField('boxart'), 'artwork_path');
});
test('patches stage approved presentation fields only; player counts and controls stay intact', () => {
  const form = { elements: { description: { value: 'manual' }, screenshot_path: { value: 'manual.jpg' },
    player_count: { value: '2' }, control_notes: { value: 'six buttons' } } };
  Provider.applyPatch(form, { description: 'approved', player_count: 99, control_notes: 'wrong', rom_name: 'console' });
  assert.equal(form.elements.description.value, 'approved');
  assert.equal(form.elements.screenshot_path.value, 'manual.jpg');
  assert.equal(form.elements.player_count.value, '2');
  assert.equal(form.elements.control_notes.value, 'six buttons');
  assert.deepEqual(Provider.approvedPatch({ __proto__: {}, invalid: 'x' }), {});
});

class Element {
  constructor() { this.children = []; this.listeners = {}; this.value = ''; this.disabled = false; this.textContent = ''; this.innerHTML = ''; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  setAttribute() {}
  querySelectorAll() { return this.approved || []; }
  click() { return this.listeners.click({ currentTarget: this }); }
}
function fixture(post) {
  const labels = [], elements = Object.fromEntries(['#provider-query', '#provider-results', '#provider-search'].map((id) => [id, new Element()]));
  const panel = { querySelector: (id) => elements[id] };
  const form = { isConnected: true, elements: { description: { value: 'manual' }, genre: { value: '' } }, querySelector: () => panel };
  const previousDocument = global.document, previousUrl = global.URL, previousConfirm = global.confirm;
  const revoked = [];
  global.document = { createElement: (tag) => { const element = new Element(); if (tag === 'label') labels.push(element); return element; } };
  global.URL = { revokeObjectURL: (url) => revoked.push(url), createObjectURL: () => 'blob:preview' };
  global.confirm = () => true;
  const cleanup = Provider.bind(form, { id: 'aliens', display_name: 'Aliens (World)' }, { post, escapeHtml: String });
  return { form, elements, labels, cleanup, revoked, restore() { cleanup(); global.document = previousDocument; global.URL = previousUrl; global.confirm = previousConfirm; } };
}
test('search sends title only, displays explicit matches and never launches or imports automatically', async () => {
  const calls = [];
  const s = fixture(async (url, body) => { calls.push([url, body]); return { candidates: [{ id: 1, title: 'Aliens', release_year: 1990 }] }; });
  try {
    await s.elements['#provider-search'].click();
    assert.deepEqual(calls, [['/api/v1/metadata/thegamesdb/search', { query: 'Aliens' }]]);
    assert.equal(s.elements['#provider-results'].children[1].textContent, 'Aliens · 1990 · Arcade · #1');
    assert.equal(s.form.elements.description.value, 'manual');
  } finally { s.restore(); }
});
test('late results cannot update a closed or replaced editor', async () => {
  let resolve;
  const s = fixture(() => new Promise((done) => { resolve = done; }));
  try {
    const pending = s.elements['#provider-search'].click();
    s.cleanup(); s.form.isConnected = false;
    resolve({ candidates: [{ id: 1, title: 'old' }] }); await pending;
    assert.equal(s.elements['#provider-results'].children.length, 0);
  } finally { s.restore(); }
});
test('Enter in provider search cannot submit the metadata form', async () => {
  let calls = 0, prevented = false;
  const s = fixture(async () => { calls++; return { candidates: [] }; });
  try {
    s.elements['#provider-query'].listeners.keydown({ key: 'Enter', preventDefault() { prevented = true; } });
    await Promise.resolve();
    assert.equal(prevented, true); assert.equal(calls, 1);
  } finally { s.restore(); }
});
test('review leaves existing fields unchecked, empty fields checked and requires explicit staging then save', async () => {
  const calls = [];
  const s = fixture(async (url, body) => {
    calls.push([url, body]);
    if (url.endsWith('/search')) return { candidates: [{ id: 1, title: 'Aliens' }] };
    if (url.endsWith('/review')) return { title: 'Aliens', metadata: { description: 'provider', genre: 'Action' }, images: [] };
    return { patch: { description: 'approved' } };
  });
  try {
    await s.elements['#provider-search'].click();
    await s.elements['#provider-results'].children[1].click();
    assert.doesNotMatch(s.labels[0].innerHTML, /checked/);
    assert.match(s.labels[1].innerHTML, /checked/);
    const results = s.elements['#provider-results'];
    results.querySelectorAll = (selector) => selector.includes('field') ? [{ dataset: { providerField: 'description' } }] : [];
    const stage = results.children.find((child) => child.textContent === 'Apply approved choices to editor');
    await stage.click();
    assert.deepEqual(calls[2], ['/api/v1/games/aliens/metadata/thegamesdb/stage', { id: 1, fields: ['description'], images: [] }]);
    assert.equal(s.form.elements.description.value, 'approved');
    assert.match(results.children.at(-1).textContent, /Save metadata/);
    assert.equal(calls.length, 3); // No hidden metadata PUT.
  } finally { s.restore(); }
});
test('edits made during staging cannot be overwritten by a late patch', async () => {
  let finishStage;
  const s = fixture(async (url) => {
    if (url.endsWith('/search')) return { candidates: [{ id: 1, title: 'Aliens' }] };
    if (url.endsWith('/review')) return { title: 'Aliens', metadata: { description: 'provider' }, images: [] };
    return new Promise((resolve) => { finishStage = resolve; });
  });
  try {
    await s.elements['#provider-search'].click(); await s.elements['#provider-results'].children[1].click();
    const results = s.elements['#provider-results'];
    results.querySelectorAll = (selector) => selector.includes('field') ? [{ dataset: { providerField: 'description' } }] : [];
    const pending = results.children.find((child) => child.textContent === 'Apply approved choices to editor').click();
    s.form.elements.description.value = 'newly typed';
    finishStage({ patch: { description: 'provider' } }); await pending;
    assert.equal(s.form.elements.description.value, 'newly typed');
    assert.match(results.children.at(-1).textContent, /No staged values were applied/);
  } finally { s.restore(); }
});
test('cancelled approval of an existing field makes no staging request', async () => {
  let calls = 0;
  const s = fixture(async (url) => {
    calls++;
    return url.endsWith('/search') ? { candidates: [{ id: 1, title: 'Aliens' }] } : { title: 'Aliens', metadata: { description: 'provider' }, images: [] };
  });
  try {
    await s.elements['#provider-search'].click(); await s.elements['#provider-results'].children[1].click();
    const results = s.elements['#provider-results'];
    results.querySelectorAll = (selector) => selector.includes('field') ? [{ dataset: { providerField: 'description' } }] : [];
    global.confirm = () => false;
    await results.children.find((child) => child.textContent === 'Apply approved choices to editor').click();
    assert.equal(calls, 2); assert.equal(s.form.elements.description.value, 'manual');
  } finally { s.restore(); }
});
