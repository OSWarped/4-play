const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs'), vm = require('node:vm');
const Upload = require('../control-plane/server/static/admin/asset-upload.js');
const jpeg = { name: 'screenshot.jpg', type: 'image/jpeg', size: 100 };

test('suggested destinations use file format and selected metadata field', () => {
  assert.equal(Upload.suggestedPath('aliens', 'screenshot_path', jpeg), 'media/aliens/screenshot.jpg');
  assert.equal(Upload.suggestedPath('kinst', 'logo_path', { name: 'logo.png', type: '', size: 100 }), 'media/kinst/logo.png');
  assert.throws(() => Upload.suggestedPath('../bad', 'logo_path', jpeg));
  assert.throws(() => Upload.suggestedPath('aliens', 'not_a_field', jpeg));
});
test('JPEG cannot be mislabeled SVG or PNG; JPEG aliases are accepted', () => {
  for (const path of ['media/aliens/screenshot.svg', 'media/aliens/screenshot.png']) {
    assert.throws(() => Upload.validateUpload(path, jpeg), /Changing a filename does not convert/);
  }
  Upload.validateUpload('media/aliens/screenshot.JPEG', jpeg);
  Upload.validateUpload('media/aliens/screenshot.jpg', { ...jpeg, type: '' });
});
test('safe paths use the shipped protected API, correctly encoding spaces', () => {
  assert.equal(Upload.assetUrl('media/kinst/7873-1 (1).jpg'), '/api/v1/assets/media/kinst/7873-1%20(1).jpg');
  for (const path of ['', '/image.jpg', 'media/../image.jpg', 'https://elsewhere/image.jpg', 'a\\b', 'a//b']) {
    assert.throws(() => Upload.assetUrl(path));
  }
});
test('size and unsupported image errors are actionable; generic media remains supported', () => {
  assert.throws(() => Upload.validateUpload('a.jpg', { ...jpeg, size: 0 }), /nonempty/);
  assert.throws(() => Upload.validateUpload('a.jpg', { ...jpeg, size: 10 * 1024 * 1024 + 1 }), /10 MiB/);
  assert.throws(() => Upload.validateUpload('a.bmp', { name: 'a.bmp', type: 'image/bmp', size: 10 }), /JPEG, PNG/);
  Upload.validateUpload('a.jpg', { ...jpeg, size: 10 * 1024 * 1024 });
  Upload.validateUpload('a.webm', { name: 'a.webm', type: 'video/webm', size: 10 });
});

function setup(path, exists = false, confirm = true) {
  const calls = [], field = { value: 'old.jpg' }, result = { innerHTML: '' };
  const form = { elements: { screenshot_path: field }, querySelector: (id) => ({
    '#asset-upload-path': { value: path }, '#asset-upload-field': { value: 'screenshot_path' },
    '#asset-upload-file': { files: [jpeg] }, '#asset-upload-result': result,
  })[id] };
  const context = vm.createContext({ FourPlayAssetUpload: Upload, escapeHtml: (v) => v,
    window: { confirm: () => confirm },
    authHeaders: () => ({ Authorization: 'Bearer test-only' }),
    fetch: async (url, options) => { calls.push(['exists', url, options.headers]); return { ok: exists, status: 404 }; },
    putRaw: async (url) => { calls.push(['put', url]); },
  });
  const source = fs.readFileSync(require.resolve('../control-plane/server/static/admin/app.js'), 'utf8');
  const start = source.indexOf('async function uploadAsset(');
  const end = source.indexOf('function metadataFromForm(', start);
  vm.runInContext(source.slice(start, end) + '\nfunction assetUrl(path) { return FourPlayAssetUpload.assetUrl(path); }', context);
  return { context, form, calls, result, field };
}
test('shipped uploader rejects mismatched extension before any request or metadata change', async () => {
  const s = setup('media/aliens/screenshot.svg'); await s.context.uploadAsset(s.form);
  assert.equal(s.calls.length, 0); assert.equal(s.field.value, 'old.jpg');
  assert.match(s.result.innerHTML, /needs a .jpg destination/);
});
test('shipped overwrite check is authenticated and cancellation preserves the asset/reference', async () => {
  const s = setup('media/aliens/screenshot.jpg', true, false); await s.context.uploadAsset(s.form);
  assert.equal(s.calls.length, 1);
  assert.equal(s.calls[0][1], '/api/v1/assets/media/aliens/screenshot.jpg');
  assert.equal(s.calls[0][2].Authorization, 'Bearer test-only');
  assert.equal(s.field.value, 'old.jpg');
  assert.match(s.result.innerHTML, /cancelled/);
});
test('successful upload applies path only to chosen field and reminds operator to save metadata', async () => {
  const s = setup('media/aliens/screenshot.jpg'); await s.context.uploadAsset(s.form);
  assert.equal(s.calls[1][0], 'put');
  assert.equal(s.field.value, 'media/aliens/screenshot.jpg');
  assert.match(s.result.innerHTML, /Save metadata/);
});
