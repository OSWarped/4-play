(function (root) {
  const imageTypes = { 'image/jpeg': ['jpg', 'jpeg'], 'image/png': ['png'],
    'image/gif': ['gif'], 'image/webp': ['webp'], 'image/svg+xml': ['svg'] };
  function extension(file) {
    const known = imageTypes[file.type?.toLowerCase()];
    if (known) return known[0];
    return String(file.name || '').split('.').pop().toLowerCase();
  }
  function suggestedPath(gameId, field, file) {
    if (!/^[a-z0-9_-]+$/i.test(gameId) || !['artwork_path', 'marquee_path', 'screenshot_path', 'logo_path'].includes(field)) {
      throw new Error('Invalid game or artwork field.');
    }
    return `media/${gameId}/${field.replace('_path', '')}.${extension(file)}`;
  }
  function assetUrl(path) {
    if (!path || path.includes('..') || /[:\\\x00-\x1f]/.test(path) || path.split('/').some((part) => !part)) {
      throw new Error('Use a relative asset path, such as media/aliens/screenshot.jpg.');
    }
    return '/api/v1/assets/' + path.split('/').map(encodeURIComponent).join('/');
  }
  function validateUpload(path, file) {
    assetUrl(path);
    if (!file || file.size === 0) throw new Error('Choose a nonempty file.');
    if (file.size > 10 * 1024 * 1024) throw new Error('Files must be 10 MiB or smaller.');
    const ext = path.split('.').pop().toLowerCase();
    const extensions = imageTypes[file.type?.toLowerCase()] ||
      Object.values(imageTypes).find((values) => values.includes(extension(file)));
    if (extensions && !extensions.includes(ext)) {
      throw new Error(`This image needs a .${extensions[0]} destination, not .${ext}. Changing a filename does not convert the image.`);
    }
    if (file.type?.startsWith('image/') && !extensions) {
      throw new Error('Use a JPEG, PNG, GIF, WebP or SVG image.');
    }
  }
  const api = { assetUrl, suggestedPath, validateUpload };
  root.FourPlayAssetUpload = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof window !== 'undefined' ? window : globalThis);
