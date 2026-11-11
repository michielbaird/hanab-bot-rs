import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizeExport, toOldExport } from "./exportFormat";

const NEW_EXPORT = {
  gameId: 2045738,
  seed: "p3v0s1",
  playerNames: ["Alice", "Bob", "Cathy"],
  deck: [{ suitIndex: 0, rank: 1 }],
  options: { variantName: "Rainbow (5 Suits)", deckPlays: true },
  actions: [
    { type: "play", order: 5 },
    { type: "discard", order: 6 },
    { type: "clue", target: 1, clue: { type: "color", value: 3 } },
    { type: "clue", target: 2, clue: { type: "rank", value: 5 } },
    { type: "gameOver", endCondition: "terminatedByVote", playerIndex: 1 },
  ],
};

test("turns a new export into the old shape", () => {
  assert.deepEqual(toOldExport(NEW_EXPORT), {
    id: 2045738,
    seed: "p3v0s1",
    players: ["Alice", "Bob", "Cathy"],
    deck: [{ suitIndex: 0, rank: 1 }],
    options: { variant: "Rainbow (5 Suits)", deckPlays: true },
    actions: [
      { type: 0, target: 5, value: 0 },
      { type: 1, target: 6, value: 0 },
      { type: 2, target: 1, value: 3 },
      { type: 3, target: 2, value: 5 },
      { type: 4, target: 1, value: 10 },
    ],
  });
});

test("an export without options is the default variant", () => {
  const old = toOldExport({
    playerNames: ["a", "b"],
    deck: [],
    actions: [],
  }) as {
    options: { variant: string };
  };
  assert.equal(old.options.variant, "No Variant");
});

test("an old export is left as it is", () => {
  const old = {
    players: ["a", "b"],
    deck: [],
    actions: [{ type: 0, target: 1, value: 0 }],
    options: { variant: "No Variant" },
  };
  assert.deepEqual(toOldExport(old), old);
});

test("a turn that the bot does not know is an error", () => {
  assert.throws(() =>
    toOldExport({ playerNames: ["a"], actions: [{ type: "dance" }] }),
  );
});

test("works on the text that the site sends", () => {
  const text = normalizeExport(JSON.stringify(NEW_EXPORT));
  assert.equal(JSON.parse(text).players.length, 3);
});
