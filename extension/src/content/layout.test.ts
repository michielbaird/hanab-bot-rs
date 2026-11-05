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

test("the robot goes in free space in the bottom row, the jump buttons beside the controls", () => {
  const { robot, next, previous } = getButtonPositions(geometry);
  assert.ok(previous.left < next.left);
  assert.ok(next.left + 28 <= geometry.controls.x);
  assert.ok(robot.left + 28 <= geometry.bottomRow[0]!.x);

  // A shared replay: buttons on both sides of a small gap.
  const shared = getButtonPositions({
    ...geometry,
    bottomRow: [
      { x: 70, y: 420, width: 250, height: 40 },
      { x: 360, y: 420, width: 250, height: 40 },
    ],
  });
  assert.ok(shared.robot.left >= 320 && shared.robot.left + 28 <= 360);
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
