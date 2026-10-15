/* Upload files supplied by the administrator. Never search/download ROMs. */
(function(root) {
  const CHUNK = 4 * 1024 * 1024;
  function matchFile(file, options, forcedKind) {
    const name = file.name.toLowerCase();
    const relative = (file.webkitRelativePath || '').replaceAll('\\', '/').toLowerCase();
    const kind = forcedKind || (name.endsWith('.chd') ? 'chd' : /(^|\/)samples\//.test(relative) ? 'samples' : 'rom');
    const candidates = options.filter(option => !option.installed && option.kind === kind && option.path.split('/').at(-1) === name);
    if (candidates.length === 1) return candidates[0];
    return candidates.find(option => relative.endsWith('/' + option.path)) || null;
  }
  const api = { matchFile, CHUNK };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.FourPlayGameImport = api;
  api.bind = ({ panel, request, refreshed }) => {
    const find = id => panel.querySelector('#' + id);
    let selected = [], job = null, busy = false, poll = null;
    const status = text => { find('game-import-result').textContent = text; };
    const display = value => {
      job = value;
      localStorage.setItem('fourplay.gameImportJob', job.id);
      find('game-import-install').disabled = job.state !== 'ready' || busy;
      status([`${job.plan.title}: ${job.state}`, `MAME support: ${job.plan.driver_status}`, job.error || '',
        job.plan.note, ...job.plan.files.filter(item => !item.installed).map(item => `${item.role || item.kind}: ${item.path}`),
        job.audit?.output || '', job.audit?.sample_output || '', ...(job.audit?.warnings || []), job.message || ''].filter(Boolean).join('\n'));
    };
    const setBusy = value => {
      busy = value;
      for (const id of ['game-import-inspect','game-import-upload','game-import-discard']) find(id).disabled = value;
      find('game-import-upload').disabled = value || !job || job.plan.driver_status === 'preliminary' || ['installed','discarded'].includes(job.state);
      find('game-import-install').disabled = value || job?.state !== 'ready';
    };
    const watch = () => {
      clearTimeout(poll);
      poll = setTimeout(async () => {
        try {
          const value = await request(`/api/v1/admin/game-imports/${job.id}`);
          display(value);
          if (['auditing','installing'].includes(value.state)) watch();
          else { setBusy(false); if (value.state === 'installed') { refreshed(); root.setTimeout(refreshed, 10000); } }
        } catch(error) { setBusy(false); status(`Status check failed. Reopen this import to retry: ${error.message}`); }
      }, 1000);
    };
    for (const id of ['game-import-files','game-import-folder']) find(id).addEventListener('change', event => {
      selected = Array.from(event.target.files || []);
      find('game-import-names').replaceChildren(...selected.filter(file => /\.(zip|7z)$/i.test(file.name)).map(file => {
        const option = root.document.createElement('option'); option.value = file.name.replace(/\.(zip|7z)$/i,'').toLowerCase(); return option;
      }));
      if (job && ['installed','discarded'].includes(job.state)) find('game-import-name').value = '';
      const archive = selected.find(file => /\.(zip|7z)$/i.test(file.name));
      if (!find('game-import-name').value && archive) find('game-import-name').value = archive.name.replace(/\.(zip|7z)$/i,'').toLowerCase();
      status(`${selected.length} files selected. Only files matching the inspected game/dependencies will be staged; artwork and DAT files are ignored.`);
    });
    let sampleFiles = [];
    find('game-import-samples').addEventListener('change', event => { sampleFiles = Array.from(event.target.files || []); status(`${sampleFiles.length} optional sample packs selected. QSound/BIOS files belong in the regular file picker.`); });
    find('game-import-inspect').addEventListener('click', async () => {
      if(busy) return; setBusy(true);
      try {
        const game = find('game-import-name').value.trim().toLowerCase();
        if(job && !['installed','discarded'].includes(job.state)) {
          if(job.plan.game === game) { display(await request(`/api/v1/admin/game-imports/${job.id}`)); return; }
          if(!root.confirm('Discard the previous staged import before inspecting another game? Installed files are never removed.')) return;
          await request(`/api/v1/admin/game-imports/${job.id}/discard`,{});
        }
        display(await request('/api/v1/admin/game-imports',{game}));
      }
      catch(error) { status(error.message); } finally { setBusy(false); }
    });
    find('game-import-upload').addEventListener('click', async () => {
      if(busy || !job) return; setBusy(true);
      try {
        display(await request(`/api/v1/admin/game-imports/${job.id}`));
        for (const file of [...selected, ...sampleFiles]) {
          const option = matchFile(file,job.plan.files,sampleFiles.includes(file) ? 'samples' : undefined);
          if(!option) continue;
          let item = job.uploads.find(item => item.path === option.path && item.kind === option.kind);
          if(!item) { display(await request(`/api/v1/admin/game-imports/${job.id}/files`,{path:option.path,kind:option.kind,size:file.size})); item=job.uploads.find(item=>item.path===option.path && item.kind===option.kind); }
          if(item.size !== file.size) throw Error(`Reselected file size differs: ${file.name}. Discard this draft and start again.`);
          for(let offset=item.received || 0;offset<file.size;) {
            const chunk=file.slice(offset,offset+CHUNK);
            const result=await request(`/api/v1/admin/game-imports/${job.id}/files/${item.id}/${offset}`,chunk,'PUT');
            offset=result.received; status(`Uploading ${file.name}: ${Math.round(offset/file.size*100)}%`);
          }
        }
        display(await request(`/api/v1/admin/game-imports/${job.id}/audit`,{})); watch();
      } catch(error) { setBusy(false); status(error.message); }
    });
    find('game-import-install').addEventListener('click', async () => {
      if(busy || job?.state!=='ready' || !root.confirm('Install verified files and add this game to the runtime catalog? Existing files will not be overwritten.')) return;
      setBusy(true);
      try { display(await request(`/api/v1/admin/game-imports/${job.id}/install`,{})); watch(); }
      catch(error) { setBusy(false); status(error.message); }
    });
    find('game-import-discard').addEventListener('click', async () => {
      if(busy || !job || !root.confirm('Delete staged copies for this import? Installed files will not be removed.')) return;
      try { display(await request(`/api/v1/admin/game-imports/${job.id}/discard`,{})); localStorage.removeItem('fourplay.gameImportJob'); }
      catch(error) { status(error.message); }
    });
    api.load = async () => {
      try {
        const settings=await request('/api/v1/admin/game-imports');
        panel.querySelector('fieldset').disabled=!settings.enabled;
        if(!settings.enabled) { status('Uploads are disabled until the operator configures the import worker and library paths.'); return; }
        const previous=localStorage.getItem('fourplay.gameImportJob');
        if(previous) { display(await request(`/api/v1/admin/game-imports/${previous}`)); if(['auditing','installing'].includes(job.state)) {setBusy(true); watch();} }
        else status(`Ready to import into ${settings.host_id}. Inspect a game before uploading.`);
      } catch(error) { status(error.message); }
    };
    return api.load;
  };
})(typeof window === 'undefined' ? globalThis : window);
