import assert from "node:assert/strict";
import { test } from "node:test";
import {
  commentsAt,
  getAnalyzable,
  getSegment,
  getStops,
  getSummary,
  getSupportKey,
} from "./analysis";
import { analysis, info } from "./fixtures";

test("a comment is shown on the position before its move", () => {
  assert.equal(getSegment({ turn: 4, player: 0, mistake: false, text: "" }), 3);
});

test("the stops are the commented turns in order, then the end of the game", () => {
  assert.deepEqual(getStops(analysis, 72), [3, 8, 19, 72]);
  assert.deepEqual(getStops({ players: [], comments: [] }, 10), [10]);
});

test("comments at a segment", () => {
  assert.deepEqual(
    commentsAt(analysis, 8).map((c) => c.turn),
    [9],
  );
  assert.deepEqual(commentsAt(analysis, 5), []);
});

test("the summary counts moves and clear mistakes", () => {
  assert.match(
    getSummary(analysis, "HGroup3"),
    /\(HGroup3\): 3 moves differ.*1 clear mistake\./,
  );
  assert.match(
    getSummary({ players: [], comments: [analysis.comments[1]!] }, "X"),
    /1 move differs.*0 clear mistakes/,
  );
  assert.match(
    getSummary({ players: [], comments: [] }, "X"),
    /would not have played any move differently/,
  );
});

test("only finished games in the database can be analyzed", () => {
  assert.equal(getAnalyzable(null), null);
  assert.equal(getAnalyzable({ ...info, finished: false }), null);
  assert.equal(getAnalyzable({ ...info, databaseID: null }), null);
  assert.equal(getAnalyzable({ ...info, databaseID: 0 }), null);
  assert.equal(getAnalyzable(info)?.databaseID, 7);
});

test("the support key depends on the variant and the options, not on the replay position", () => {
  assert.equal(getSupportKey(info), getSupportKey({ ...info, segment: 40 }));
  assert.notEqual(
    getSupportKey(info),
    getSupportKey({ ...info, variant: "Blind (5 Suits)" }),
  );
  assert.notEqual(
    getSupportKey(info),
    getSupportKey({ ...info, emptyClues: true }),
  );
});
