import assert from "node:assert/strict";
import { test } from "node:test";
import { readReplay } from "./pageState";

test("reads the replay of the site as of November 2026", () => {
  const replay = readReplay({ segment: 12, gameId: 2045738 });
  assert.deepEqual(replay, {
    gameId: 2045738,
    segment: 12,
    active: true,
    inHypothetical: false,
    shared: null,
  });
});

test("outside a replay the segment is absent", () => {
  const replay = readReplay({ gameId: 7 });
  assert.equal(replay.active, false);
  assert.equal(replay.segment, 0);
});

test("a game that is not saved has no id", () => {
  assert.equal(readReplay({ segment: 0 }).gameId, null);
});

test("reads the replay of the site of before", () => {
  const replay = readReplay({
    active: true,
    segment: 3,
    databaseID: 99,
    hypothetical: null,
    shared: null,
  });
  assert.equal(replay.gameId, 99);
  assert.equal(replay.active, true);
  assert.equal(replay.inHypothetical, false);
});

test("a hypothetical and a shared replay are noticed in either shape", () => {
  assert.equal(
    readReplay({ segment: 0, hypothetical: {} }).inHypothetical,
    true,
  );
  assert.equal(
    readReplay({ segment: 0, hypothetical: undefined }).inHypothetical,
    false,
  );
  const shared = { useSharedSegments: true, amLeader: false };
  assert.deepEqual(readReplay({ segment: 0, shared }).shared, shared);
});
