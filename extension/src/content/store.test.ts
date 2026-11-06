import assert from "node:assert/strict";
import { test } from "node:test";
import { createStore } from "./store";

test("listeners hear about changes, not about nothing changing", () => {
  const store = createStore({ a: 1, b: { x: 1 } });
  const seen: number[] = [];
  store.subscribe((s) => seen.push(s.a));

  store.update({ a: 2 });
  store.update({ a: 2 });
  store.update((s) => ({ a: s.a + 1 }));
  assert.deepEqual(seen, [2, 3]);
  assert.equal(store.get().a, 3);
});

test("a listener that changes the state sees the newest state afterwards", () => {
  const store = createStore({ a: 1, b: 0 });
  const seen: Array<[number, number]> = [];
  store.subscribe((s) => {
    if (s.a === 2 && s.b === 0) store.update({ b: 1 });
  });
  store.subscribe((s) => seen.push([s.a, s.b]));

  store.update({ a: 2 });
  assert.deepEqual(seen.at(-1), [2, 1]);
});
