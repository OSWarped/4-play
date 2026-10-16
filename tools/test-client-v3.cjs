const { test } = require('node:test');
const assert = require('node:assert/strict');
const V3 = require('../control-plane/server/static/client-v3/experience.js');
test('V3 genre and live filters compose without mutating the catalog', () => {
  const games = [
    { id: 'a', metadata: { genre: ' Fighting ' } },
    { id: 'b', metadata: { genre: 'Beat em up' } },
    { id: 'c', metadata: { genre: 'Fighting' } }, { id: 'd' },
  ];
  const sessions = [{ game_id: 'a', state: 'active' }, { game_id: 'b', state: 'launching' }];
  assert.deepEqual(V3.genres(games), ['Beat em up', 'Fighting']);
  assert.deepEqual(V3.filterGames(games, 'Fighting').map(game => game.id), ['a', 'c']);
  assert.deepEqual(V3.filterGames(games, '', true, sessions).map(game => game.id), ['a']);
  assert.deepEqual(V3.filterGames(games, 'Beat em up', true, sessions), []);
  assert.equal(V3.filterGames(games).length, 4);
  assert.equal(games[0].metadata.genre, ' Fighting ');
});
