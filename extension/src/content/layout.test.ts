import assert from "node:assert/strict";
import { test } from "node:test";
import { getSegment } from "./analysis";
import { analysis, geometry, info } from "./fixtures";
import {
  getButtonPositions,
  getCommentBox,
  getMarkers,
  MISTAKE_COLOR,
  SUMMARY_COLOR,
} from "./layout";

test('the buttons are left of the "Enter Hypothetical" button, or right of it when there is no room', () => {
  const { robot, next, previous } = getButtonPositions(geometry);
  assert.ok(previous.left < next.left && next.left < robot.left);
  assert.equal(robot.top, next.top);
  assert.ok(robot.left + 28 <= geometry.hypoButton.x);

  const cramped = getButtonPositions({
    ...geometry,
    hypoButton: { ...geometry.hypoButton, x: geometry.replayArea.x },
  });
  assert.ok(
    cramped.previous.left >= geometry.replayArea.x + geometry.hypoButton.width,
  );
});

test("there is a marker for every comment and one for the end of the game", () => {
  const markers = getMarkers(geometry, info, analysis);
  assert.equal(markers.length, 4);
  assert.deepEqual(
    markers.map((m) => m.segment).toSorted((a, b) => a - b),
    [3, 8, 19, 72],
  );
  assert.equal(markers.at(-1)?.color, SUMMARY_COLOR);
});

test("mistakes are drawn after (on top of) suggestions", () => {
  const comments = getMarkers(geometry, info, analysis).slice(0, -1);
  assert.equal(comments.at(-1)?.color, MISTAKE_COLOR);
});

test("the markers run along the bar without covering the shuttle", () => {
  const markers = getMarkers(geometry, info, analysis);
  const first = markers.find((m) => m.segment === 3)!;
  const last = markers.find((m) => m.segment === 72)!;
  assert.ok(first.left < last.left);
  const { bar, shuttle } = geometry;
  assert.ok(last.left + last.width <= bar.x + bar.width);
  assert.ok(
    first.top + first.height <= bar.y + bar.height / 2 - shuttle.height / 2,
  );
});

test("a long comment in a small window gets a smaller font, within limits", () => {
  const long = {
    ...analysis,
    comments: [{ ...analysis.comments[0]!, text: "x".repeat(120) }],
  };
  const at = (width: number) =>
    getCommentBox(
      { ...geometry, replayArea: { ...geometry.replayArea, width } },
      { ...info, segment: getSegment(long.comments[0]!) },
      long,
      "X",
    )!.fontSize;
  assert.ok(at(500) < 13);
  assert.equal(at(100), 9);
  assert.equal(at(5000), 13);
});

test("the comment box shows the comments of the turn, or the summary at the end", () => {
  const turn = getCommentBox(
    geometry,
    { ...info, segment: 8 },
    analysis,
    "H-Group (level 3)",
  );
  assert.equal(turn?.text, "Bob: mistake! prefer (a) over (b)");
  assert.equal(turn?.mistake, true);
  assert.equal(turn?.fontSize, 13, "a short comment is at full size");

  const end = getCommentBox(
    geometry,
    { ...info, segment: 72 },
    analysis,
    "H-Group (level 3)",
  );
  assert.match(end?.text ?? "", /3 moves differ/);
  assert.equal(end?.mistake, false);

  assert.equal(
    getCommentBox(geometry, { ...info, segment: 5 }, analysis, "X"),
    null,
  );
});
