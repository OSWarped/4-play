(function (root) {
  const descriptiveFields = ['sort_title', 'description', 'genre', 'release_year', 'manufacturer'];
  const imageFields = ['artwork_path', 'screenshot_path', 'marquee_path', 'logo_path'];
  function searchTitle(title) { return String(title || '').replace(/\s*\([^)]*\)/g, '').trim(); }
  function defaultImageField(kind) {
    return ({ screenshot: 'screenshot_path', titlescreen: 'screenshot_path', clearlogo: 'logo_path', banner: 'marquee_path' })[kind] || 'artwork_path';
  }
  function approvedPatch(patch) {
    return Object.fromEntries(Object.entries(patch || {}).filter(([field]) => [...descriptiveFields, ...imageFields].includes(field)));
  }
  function applyPatch(form, patch) {
    for (const [field, value] of Object.entries(approvedPatch(patch))) {
      if (value !== null && form.elements[field]) form.elements[field].value = String(value);
    }
  }
  function bind(form, game, api) {
    const panel = form.querySelector('#provider-panel');
    const query = panel.querySelector('#provider-query');
    const results = panel.querySelector('#provider-results');
    const button = panel.querySelector('#provider-search');
    const readiness = panel.querySelector('#provider-status');
    query.value = searchTitle(game.display_name || game.id);
    query.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') { event.preventDefault(); if (!button.disabled) button.click(); }
    });
    let version = 0, busy = false;
    const urls = new Set();
    function clearImages() { for (const url of urls) root.URL.revokeObjectURL(url); urls.clear(); }
    const escape = api.escapeHtml;
    if (api.fetch && readiness) {
      api.fetch('/api/v1/metadata/thegamesdb/status', { headers: api.headers() }).then(async (response) => {
        if (!response.ok) throw new Error('Provider configuration could not be checked.');
        const status = await response.json();
        if (!form.isConnected) return;
        readiness.textContent = status.configured ? `TheGamesDB is configured on the server. Seven-day cache.${status.last_live_monthly_allowance === null ? '' : ` Last live monthly allowance: ${status.last_live_monthly_allowance} (not a fresh quota check).`}` : 'TheGamesDB is not configured. Set FOURPLAY_THEGAMESDB_API_KEY on the Linux server; do not enter that key in this browser.';
      }).catch((error) => { if (form.isConnected) readiness.textContent = String(error); });
    }
    function live(request) { return version === request && form.isConnected; }
    function errorMessage(error) { results.innerHTML = `<p class="pill bad">${escape(String(error))}</p>`; }
    button.addEventListener('click', async () => {
      if (busy) return;
      busy = true; button.disabled = true;
      const request = ++version; clearImages();
      results.textContent = 'Searching TheGamesDB Arcade catalog…';
      try {
        const response = await api.post('/api/v1/metadata/thegamesdb/search', { query: query.value.trim() });
        if (!live(request)) return;
        results.replaceChildren();
        const note = root.document.createElement('p'); note.className = 'muted';
        note.textContent = `${response.note || ''} Choose a match to review. No metadata has changed.`; results.append(note);
        for (const candidate of response.candidates || []) {
          const match = root.document.createElement('button'); match.type = 'button';
          match.textContent = `${candidate.title} · ${candidate.release_year || 'year unknown'} · Arcade · #${candidate.id}`;
          match.addEventListener('click', () => review(candidate.id)); results.append(match);
        }
        if (!response.candidates?.length) results.append('No arcade matches. Try a shorter title.');
      } catch (error) { if (live(request)) errorMessage(error); }
      finally { busy = false; button.disabled = false; }
    });
    async function review(id) {
      if (busy) return;
      busy = true; button.disabled = true;
      const request = ++version; clearImages();
      results.textContent = 'Loading candidate metadata and artwork choices…';
      try {
        const candidate = await api.post('/api/v1/metadata/thegamesdb/review', { id });
        if (!live(request)) return;
        results.innerHTML = `<h4>${escape(candidate.title)} · Arcade · TheGamesDB #${escape(id)}</h4>
          <p class="muted">Existing values stay unchecked. Select only changes you approve. Player counts, controls and ROM identity are never imported.</p>`;
        for (const field of descriptiveFields) {
          const value = candidate.metadata?.[field];
          if (value === null || value === undefined || value === '') continue;
          const current = form.elements[field]?.value.trim() || '';
          const row = root.document.createElement('label'); row.className = 'provider-field';
          row.innerHTML = `<input type="checkbox" data-provider-field="${field}" ${current ? '' : 'checked'} />
            <span><strong>${escape(field.replaceAll('_', ' '))}</strong><small>Current: ${escape(current || '(empty)')}</small><span>${escape(String(value))}</span></span>`;
          results.append(row);
        }
        const media = root.document.createElement('div'); media.className = 'provider-images';
        for (const [index, image] of (candidate.images || []).entries()) {
          let previewUrl = null;
          const row = root.document.createElement('div'); row.className = 'provider-image';
          row.innerHTML = `<p>${escape(image.kind)} ${escape(image.side || '')}</p>
            <button type="button" data-preview>Preview image</button><div data-image-preview></div>
            <label><input type="checkbox" data-provider-image="${index}" /> Import this image</label>
            <select aria-label="Destination for image ${index + 1}">${imageFields.map((field) => `<option value="${field}" ${field === defaultImageField(image.kind) ? 'selected' : ''}>${escape(field.replace('_path', ''))}</option>`).join('')}</select>`;
          row.querySelector('[data-preview]').addEventListener('click', async (event) => {
            const previewButton = event.currentTarget;
            previewButton.disabled = true;
            const output = row.querySelector('[data-image-preview]');
            try {
              const response = await api.fetch('/api/v1/metadata/thegamesdb/image', { method: 'POST', headers: { ...api.headers(), 'Content-Type': 'application/json' }, body: JSON.stringify({ id, index }) });
              if (!response.ok) throw new Error(`Image preview failed (${response.status}).`);
              const blob = await response.blob();
              if (!live(request)) return;
              const url = root.URL.createObjectURL(blob); urls.add(url);
              if (previewUrl) { root.URL.revokeObjectURL(previewUrl); urls.delete(previewUrl); }
              previewUrl = url;
              const preview = root.document.createElement('img'); preview.src = url; preview.alt = `${candidate.title} ${image.kind}`;
              output.replaceChildren(preview);
            } catch (error) { if (live(request)) output.textContent = String(error); }
            finally { if (live(request)) previewButton.disabled = false; }
          });
          media.append(row);
        }
        results.append(media);
        const stage = root.document.createElement('button'); stage.type = 'button'; stage.textContent = 'Apply approved choices to editor';
        stage.addEventListener('click', async () => {
          if (busy) return;
          const fields = [...results.querySelectorAll('[data-provider-field]:checked')].map((field) => field.dataset.providerField);
          const images = [...results.querySelectorAll('[data-provider-image]:checked')].map((image) => ({ index: Number(image.dataset.providerImage), field: image.closest('.provider-image').querySelector('select').value }));
          if (!fields.length && !images.length) { status.textContent = 'Select at least one field or image.'; return; }
          if (new Set(images.map((image) => image.field)).size !== images.length) { status.textContent = 'Choose at most one image for each destination field.'; return; }
          const overwrite = [...fields, ...images.map((image) => image.field)].some((field) => form.elements[field]?.value.trim());
          if (overwrite && !root.confirm('Apply the selected changes to existing editor values? Nothing is published until Save metadata. Existing image files will be preserved.')) return;
          busy = true; button.disabled = true; stage.disabled = true;
          const expected = Object.fromEntries([...fields, ...images.map((image) => image.field)].map((field) => [field, form.elements[field]?.value]));
          status.textContent = 'Downloading approved images and staging changes…';
          try {
            const staged = await api.post(`/api/v1/games/${encodeURIComponent(game.id)}/metadata/thegamesdb/stage`, { id, fields, images });
            if (!live(request)) return;
            if (Object.entries(expected).some(([field, value]) => form.elements[field]?.value !== value)) {
              status.textContent = 'Editor values changed while staging. No staged values were applied. Review your choices and try again; downloaded assets remain staged.';
              return;
            }
            applyPatch(form, staged.patch);
            status.textContent = 'Choices applied to editor. Click Save metadata to publish. Existing image files were not overwritten.';
          } catch (error) { if (live(request)) status.textContent = String(error); }
          finally { busy = false; button.disabled = false; stage.disabled = false; }
        });
        const status = root.document.createElement('p'); status.setAttribute('role', 'status');
        results.append(stage, status);
      } catch (error) { if (live(request)) errorMessage(error); }
      finally { busy = false; button.disabled = false; }
    }
    return () => { version++; clearImages(); };
  }
  const api = { bind, searchTitle, defaultImageField, approvedPatch, applyPatch, descriptiveFields, imageFields };
  root.FourPlayMetadataEditor = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof window !== 'undefined' ? window : globalThis);
