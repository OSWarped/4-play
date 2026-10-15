/* Real Chromium geometry regression. Local fixtures only; no game/session writes.
 * node tools/check-client-coverflow-browser.cjs [path-to-chrome-or-edge]
 */
const assert = require('node:assert/strict');
const fs = require('node:fs'), http = require('node:http'), os = require('node:os'), path = require('node:path');
const { spawn } = require('node:child_process');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const base = path.resolve('control-plane/server/static');
const games = Array.from({ length: 46 }, (_, index) => ({
  id: `game-${index}`, display_name: `Original title (${index})`,
  metadata: { sort_title: index === 0 ? 'Aliens' : index === 1 ? 'Killer Instinct' : `Arcade game ${index}: a longer title`,
    genre: index % 2 ? 'Fighting' : 'Beat em up', release_year: 1990 + index % 9, manufacturer: 'Arcade classics',
    description: 'Choose your game. Join friends, watch a match, or start something new.', player_count: 2,
    ...(index % 3 ? {} : { artwork_path: 'fixture/poster.svg', screenshot_path: 'fixture/scene.svg',
      logo_path: 'fixture/logo.svg', marquee_path: 'fixture/marquee.svg' }) },
}));
async function main() {
  const browser = process.argv[2] || process.env.CHROME_PATH || 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
  assert.ok(fs.existsSync(browser), 'Pass the Chrome/Edge executable path');
  const server = http.createServer((request, response) => {
    const url = new URL(request.url, 'http://localhost');
    if (url.pathname.startsWith('/client-v2/api/assets/')) {
      const role = path.basename(url.pathname, '.svg');
      response.setHeader('Content-Type', 'image/svg+xml');
      response.end(`<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400"><defs><linearGradient id="g"><stop stop-color="#1b334f"/><stop offset="1" stop-color="#365e53"/></linearGradient></defs><rect width="640" height="400" fill="url(#g)"/><circle cx="460" cy="130" r="100" fill="#93e6c5" opacity=".35"/><text x="40" y="220" fill="white" font-size="55" font-family="sans-serif">${role === 'logo' ? 'ALIENS' : role === 'marquee' ? '4-PLAY ARCADE' : 'ARCADE CLASSICS'}</text></svg>`);
      return;
    }
    if (url.pathname === '/client-v2/bootstrap' || url.pathname.startsWith('/client-v2/api/')) {
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify(url.pathname.endsWith('/bootstrap') ? { seat_id: 'fixture', destination_address: '127.0.0.1' }
        : url.pathname.endsWith('/games') ? { games } : url.pathname.endsWith('/active-sessions') ? { sessions: [] } : {})); return;
    }
    let file = url.pathname === '/client-v2' ? 'client/index.html' : url.pathname.slice(1);
    if (!/^(client|client-v2)\/[a-z-]+\.(js|css|html)$/.test(file)) { response.writeHead(404).end(); return; }
    let content = fs.readFileSync(path.join(base, file), 'utf8');
    if (file === 'client/index.html') content = content.replace('<body>', '<body data-client-version="2">')
      .replace('</head>', '<link rel="stylesheet" href="/client-v2/styles.css" /></head>')
      .replace('<script src="/client/app.js"', '<script src="/client-v2/experience.js" defer></script><script src="/client/app.js"');
    response.setHeader('Content-Type', file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html');
    response.end(content);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), '4play-coverflow-browser-'));
  const child = spawn(browser, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', 'about:blank'], { windowsHide: true, stdio: 'ignore' });
  let ws, send;
  try {
    const portFile = path.join(profile, 'DevToolsActivePort');
    for (let n = 0; n < 100 && !fs.existsSync(portFile); n++) await pause(100);
    const port = fs.readFileSync(portFile, 'utf8').split('\n')[0];
    const page = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
    ws = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise(resolve => ws.addEventListener('open', resolve, { once: true }));
    let next = 0; const pending = new Map(), errors = [];
    ws.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const task = pending.get(message.id); pending.delete(message.id);
        if (message.error) task.reject(message.error); else task.resolve(message.result);
      } else if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails.text);
    });
    send = (method, params = {}) => new Promise((resolve, reject) => {
      const id = ++next; pending.set(id, { resolve, reject }); ws.send(JSON.stringify({ id, method, params }));
    });
    const evaluate = async expression => {
      const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails) throw Error(JSON.stringify(result.exceptionDetails));
      return result.result.value;
    };
    const assertCentered = async () => {
      const delta = await evaluate(`(()=>{const card=document.querySelector('.game-cover[aria-pressed=true]'); const rail=card.parentElement; return Math.abs(rail.scrollLeft-FourPlayCarousel.centeredLeft(rail,card));})()`);
      assert.ok(delta < 2, `Selected cover is ${delta}px away from its layout center`);
    };
    await send('Runtime.enable'); await send('Page.enable');
    await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1100, deviceScaleFactor: 1, mobile: false });
    await send('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/client-v2` });
    for (let n = 0; n < 100; n++) { if (await evaluate(`document.querySelectorAll('.game-cover').length === 46`)) break; await pause(100); }
    assert.equal(await evaluate(`document.querySelectorAll('.game-cover').length`), 46);
    for (let index = 0; index < 46; index++) {
      await evaluate(`document.querySelectorAll('.game-cover')[${index}].click()`);
      await pause(430);
      assert.equal(await evaluate(`document.querySelector('.game-cover[aria-pressed=true]').dataset.gameId`), `game-${index}`);
      assert.equal(await evaluate(`document.querySelector('#featured-title').textContent`), games[index].metadata.sort_title);
      await assertCentered();
    }
    await evaluate(`document.querySelectorAll('.game-cover')[0].click()`); await pause(700);
    await assertCentered();
    assert.deepEqual(await evaluate(`['#featured-logo','#featured-marquee','#featured-title'].map(id=>document.querySelector(id).hidden)`), [false, true, true]);
    assert.ok(await evaluate(`document.querySelector('.game-cover').offsetWidth`)>300);
    const screenshot = async name => {
      fs.mkdirSync('tmp', { recursive: true });
      fs.writeFileSync(`tmp/${name}.png`, Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).data, 'base64'));
    };
    await screenshot('client-modern-desktop');
    await evaluate(`window.oldCard = document.querySelector('.game-cover'); refresh()`); await pause(500);
    assert.equal(await evaluate(`oldCard === document.querySelector('.game-cover')`), true);
    await evaluate(`(()=>{const rail=document.querySelector('#games'); const card=document.querySelectorAll('.game-cover')[17]; rail.dispatchEvent(new Event('pointerdown')); rail.scrollTo({left:FourPlayCarousel.centeredLeft(rail,card),behavior:'instant'});})()`);
    await pause(700);
    assert.equal(await evaluate(`document.querySelector('.game-cover[aria-pressed=true]').dataset.gameId`), 'game-17');
    await assertCentered();
    for (const [width, height, name] of [[430, 932, 'phone'], [932, 430, 'landscape']]) {
      await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: true });
      await evaluate(`document.querySelectorAll('.game-cover')[23].click()`); await pause(600);
      assert.equal(await evaluate(`document.querySelector('.game-cover[aria-pressed=true]').dataset.gameId`), 'game-23');
      await assertCentered();
      assert.equal(await evaluate(`document.documentElement.scrollWidth > innerWidth`), false);
      assert.equal(await evaluate(`document.querySelector('#featured-logo').hidden && document.querySelector('#featured-marquee').hidden`), true);
      assert.equal(await evaluate(`document.querySelector('#featured-title').hidden`), false);
      await screenshot(`client-modern-${name}`);
    }
    assert.deepEqual(errors, []);
    console.log('PASS: all 46 covers hold selection; sort titles, logos/marquees, stable refresh, phone and landscape layout; no browser exceptions.');
  } finally {
    if (send && ws?.readyState === WebSocket.OPEN) await send('Browser.close').catch(() => {});
    ws?.close(); if (child.exitCode === null) child.kill();
    await new Promise(resolve => server.close(resolve));
    // Keep Chromium's temporary profile for diagnostics; never delete user profiles.
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
