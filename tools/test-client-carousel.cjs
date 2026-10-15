const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs"), vm = require("node:vm");
const { GameSelection, assetUrl, artworkPaths, ArtworkLoader, displayTitle, centeredLeft, bindRail, loadBrand } = require("../control-plane/server/static/client/carousel.js");
const games = [
  { id: "aliens", display_name: "Aliens", metadata: { genre: "Run and gun", release_year: 1990 } },
  { id: "kinst", display_name: "Killer Instinct", metadata: { genre: "Fighting", manufacturer: "Rare" } },
  { id: "tmnt", display_name: "Teenage Mutant Ninja Turtles" },
];

test("selection wraps, handles empty/single libraries and preserves refreshed identity", () => {
  const selection = new GameSelection();
  assert.equal(selection.move(1), null);
  selection.update(games);
  assert.equal(selection.move(-1).id, "tmnt");
  assert.equal(selection.move(1).id, "aliens");
  selection.select("kinst"); selection.update([...games].reverse());
  assert.equal(selection.selected.id, "kinst");
  selection.update([games[1]]); assert.equal(selection.move(1).id, "kinst");
  selection.update([]); assert.equal(selection.selected, null);
});

test("filter searches metadata, ignores unknown IDs and safely resets removed selection", () => {
  const selection = new GameSelection();
  selection.update(games, "  RARE "); assert.equal(selection.selected.id, "kinst");
  selection.select("not in library"); assert.equal(selection.selected.id, "kinst");
  selection.update(games, "1990"); assert.equal(selection.selected.id, "aliens");
  selection.update(games, "no match"); assert.equal(selection.selected, null);
  selection.update(games); assert.equal(selection.selected.id, "aliens");
});

test("asset paths are same-origin encoded relative paths, never tokens or external requests", () => {
  assert.equal(assetUrl("media/Space & Fight/art.png"), "/api/v1/assets/media/Space%20%26%20Fight/art.png");
  for (const path of ["https://evil/image", "../image", "/image", "a\\b", "a//b", "a/./b", "a\n/b"]) {
    assert.throws(() => assetUrl(path));
  }
  assert.deepEqual(artworkPaths({ metadata: { screenshot_path: "same", artwork_path: "same", logo_path: "logo" } }), ["same", "logo"]);
});

function fixture(fetch) {
  const revoked = [], requests = [], applied = [];
  let id = 0;
  const loader = new ArtworkLoader({ AbortController, fetch: (...args) => { requests.push(args); return fetch(...args); },
    URL: { createObjectURL: () => `blob:${++id}`, revokeObjectURL: (url) => revoked.push(url) } });
  return { loader, requests, applied, revoked, apply: (url) => applied.push(url) };
}
const response = (type = "image/png", size = 100) => ({ ok: true, blob: async () => ({ type, size }) });

test("artwork fetch sends bearer header, falls back after missing art, and revokes old blobs", async () => {
  let count = 0;
  const s = fixture(async () => ++count === 1 ? { ok: false } : response());
  await s.loader.load({ metadata: { screenshot_path: "missing.png", artwork_path: "found.png" } }, { Authorization: "Bearer private" }, s.apply);
  assert.equal(s.requests[0][1].headers.Authorization, "Bearer private");
  assert.equal(s.requests[0][0].includes("private"), false);
  assert.deepEqual(s.applied, [null, "blob:1"]);
  s.loader.clear(); assert.deepEqual(s.revoked, ["blob:1"]);
});

test("late artwork from a previous game cannot replace the selected game", async () => {
  let complete;
  const s = fixture(() => new Promise((resolve) => { complete = resolve; }));
  const pending = s.loader.load({ metadata: { artwork_path: "old.png" } }, {}, s.apply);
  await s.loader.load({}, {}, s.apply);
  complete(response()); await pending;
  assert.deepEqual(s.applied, [null, null]);
  assert.equal(s.requests[0][1].signal.aborted, true);
});

test("non-images, oversized images, missing files and invalid paths retain the fallback", async () => {
  for (const result of [response("text/html"), response("image/png", 17 * 1024 * 1024), { ok: false }]) {
    const s = fixture(async () => result);
    await s.loader.load({ metadata: { artwork_path: "image.png" } }, {}, s.apply);
    assert.deepEqual(s.applied, [null]);
  }
  const s = fixture(async () => { throw new Error("offline"); });
  await s.loader.load({ metadata: { artwork_path: "../bad", screenshot_path: "valid.png" } }, {}, s.apply);
  assert.deepEqual(s.applied, [null]);
  assert.equal(s.requests.length, 1);
});

test("shipped featured view updates selection, roving focus, scroll and empty-state actions", () => {
  const selection = new GameSelection(); selection.update(games);
  const elements = new Map(), covers = games.map((game) => ({
    dataset: { gameId: game.id }, attrs: {}, offsetLeft: 0, offsetWidth: 100,
    parentElement: { offsetLeft: 0, clientWidth: 300, scrollTo() {} },
    setAttribute(name, value) { this.attrs[name] = value; },
    focus() { this.focused = true; },
  }));
  const element = (id) => {
    if (!elements.has(id)) elements.set(id, { textContent: "", removeAttribute() {} });
    return elements.get(id);
  };
  const context = vm.createContext({ librarySelection: selection, libraryLaunchPending: false,
    libraryArtworkKey: null, libraryArtwork: { load: (_game, _headers, apply) => apply(null) },
    libraryLogo: { load: (_game, _headers, apply) => apply(null) },
    libraryMarquee: { load: (_game, _headers, apply) => apply(null) },
    FourPlayCarousel: { artworkPaths, displayTitle, centeredLeft, loadBrand }, authHeaders: () => ({}),
    state: { token: "", sessions: [{ game_id: "kinst" }] },
    setText: (id, value) => { element(id).textContent = value; },
    document: { querySelector: element, querySelectorAll: () => covers },
    window: { matchMedia: () => ({ matches: true }) },
  });
  const source = fs.readFileSync(require.resolve("../control-plane/server/static/client/app.js"), "utf8");
  vm.runInContext(source.slice(source.indexOf("function moveFeaturedGame("), source.indexOf("function renderSessions(")), context);
  context.updateFeaturedGame();
  assert.equal(element("#featured-title").textContent, "Aliens");
  assert.equal(covers[0].tabIndex, 0); assert.equal(covers[1].tabIndex, -1);
  context.moveFeaturedGame(1, true);
  assert.equal(element("#featured-title").textContent, "Killer Instinct");
  assert.equal(covers[1].focused, true);
  assert.match(element("#featured-running").textContent, /1 running session/);
  context.libraryLaunchPending = true; context.updateFeaturedGame();
  assert.equal(element("#featured-start").disabled, true);
  selection.update(games, "nothing matches"); context.updateFeaturedGame();
  assert.equal(element("#featured-details").disabled, true);
  assert.equal(element("#game-next").disabled, true);
  assert.equal(element("#featured-image").hidden, true);
});

test("sort titles are displayed and searchable, with trimmed fallbacks", () => {
  const game = { id: "a", display_name: "Original", metadata: { sort_title: "  Modern title  " } };
  assert.equal(displayTitle(game), "Modern title");
  assert.equal(displayTitle({ ...game, metadata: { sort_title: "  " } }), "Original");
  assert.equal(displayTitle({ id: "fallback" }), "fallback");
  const selection = new GameSelection(); selection.update([game], "modern");
  assert.equal(selection.selected.id, "a");
});

test("logos, marquees and covers have independent image priorities", async () => {
  const game = { metadata: { artwork_path: "poster.png", screenshot_path: "scene.png", logo_path: "logo.png", marquee_path: "marquee.png" } };
  assert.deepEqual(artworkPaths(game, "cover"), ["scene.png", "poster.png", "marquee.png", "logo.png"]);
  assert.deepEqual(artworkPaths(game, "logo"), ["logo.png"]);
  assert.deepEqual(artworkPaths(game, "marquee"), ["marquee.png"]);
  const s = fixture(async () => response());
  await s.loader.load(game, {}, s.apply, "logo");
  assert.equal(s.requests[0][0], "/api/v1/assets/logo.png");
});

test("a decoded logo replaces marquee/title, and failed or missing logos restore fallbacks", () => {
  const node = () => ({ hidden: false, removeAttribute() { delete this.src; } });
  const logo = node(), marquee = node(), title = node();
  const loader = role => ({ load(_game, _headers, apply) { apply(null); apply(`blob:${role}`); } });
  loadBrand({ display_name: "Original", metadata: { sort_title: "Preferred" } }, {}, loader("logo"), loader("marquee"), logo, marquee, title);
  assert.equal(title.hidden, false); // A URL alone is not proof the image decoded.
  marquee.currentSrc = "blob:marquee"; marquee.onload();
  assert.equal(marquee.hidden, false);
  logo.currentSrc = "blob:logo"; logo.onload();
  assert.equal(logo.hidden, false); assert.equal(logo.alt, "Preferred");
  assert.equal(marquee.hidden, true); assert.equal(title.hidden, true);
  logo.onerror();
  assert.equal(logo.hidden, true); assert.equal(marquee.hidden, false); assert.equal(title.hidden, false);
  const absent = { load(_game, _headers, apply) { apply(null); } };
  loadBrand({}, {}, absent, absent, logo, marquee, title);
  assert.equal(logo.hidden, true); assert.equal(marquee.hidden, true); assert.equal(title.hidden, false);
});

test("46 variable-width covers stay selectable despite arbitrary 3D bounds and scroll feedback", () => {
  const listeners = {}, timers = new Map(); let timerId = 0, selected = null;
  let offset = 120;
  const cards = Array.from({ length: 46 }, (_, index) => {
    const width = index % 7 === 0 ? 215 : 224;
    const card = { offsetLeft: offset, offsetWidth: width, dataset: { gameId: String(index) },
      getAttribute: () => selected === String(index) ? "true" : "false",
      getBoundingClientRect() { assert.fail("Transformed geometry must never determine selection"); } };
    offset += width - 21; return card;
  });
  const rail = { offsetLeft: 42, clientWidth: 800, scrollLeft: 0,
    querySelectorAll: () => cards,
    addEventListener: (event, handler) => { listeners[event] = handler; },
    scrollTo({ left }) { this.scrollLeft = left; listeners.scroll(); },
  };
  const settle = () => { for (const callback of [...timers.values()]) { timers.clear(); callback(); } };
  const env = { document: { body: { classList: { contains: () => false } } },
    clearTimeout: (id) => timers.delete(id), setTimeout: (callback) => { timers.set(++timerId, callback); return timerId; } };
  const nav = bindRail(rail, (id) => { selected = id; nav.center(cards[Number(id)], "smooth"); }, env);
  for (const card of cards) {
    selected = card.dataset.gameId;
    nav.center(card, "smooth"); settle(); settle();
    assert.equal(selected, card.dataset.gameId);
    assert.equal(rail.scrollLeft, card.offsetLeft + card.offsetWidth / 2 - 400);
  }
  // A programmatic animation passing other cards cannot replace the requested game.
  selected = "40"; nav.center(cards[40], "smooth");
  rail.scrollLeft = centeredLeft(rail, cards[17]); listeners.scroll(); settle();
  assert.equal(selected, "40");
  // A real swipe can then select that middle card and settle without bouncing.
  listeners.pointerdown(); listeners.scroll(); settle(); settle();
  assert.equal(selected, "17");
});
