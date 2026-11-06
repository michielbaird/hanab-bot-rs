import assert from "node:assert/strict";
import { test } from "node:test";
import { getSegment } from "./analysis";
import type { Geometry } from "../protocol";
import { getSiteLayout } from "../../test/site-layout.mjs";
import { analysis, geometry, info } from "./fixtures";
import {
  getButtonPositions,
  getCommentBox,
  getMarkers,
  MISTAKE_COLOR,
  SUMMARY_COLOR,
} from "./layout";

const SIZES = [
  [1920, 1080],
  [1280, 720],
  [1000, 562],
] as const;
const KINDS = ["finished", "game", "leader", "follower"] as const;

type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.width &&
  b.x < a.x + a.width &&
  a.y < b.y + b.height &&
  b.y < a.y + a.height;

for (const [width, height] of SIZES) {
  for (const kind of KINDS) {
    test(`the buttons overlap nothing of the site in a ${kind} replay at ${width}x${height}`, () => {
      const site = getSiteLayout(width, height, kind);
      const siteGeometry: Geometry = {
        bar: site.elements.replayBar!,
        shuttle: site.elements.replayShuttle!,
        replayArea: site.elements.replayArea!,
        controls: {
          ...site.controls[0]!,
          width:
            site.controls[3]!.x + site.controls[3]!.width - site.controls[0]!.x,
        },
        bottomRow: Object.values(site.bottomRow),
      };
      const { size, robot, next, previous } = getButtonPositions(siteGeometry);
      const ours = [robot, next, previous].map((p) => ({
        x: p.left,
        y: p.top,
        width: size,
        height: size,
      }));

      const comment = getCommentBox(
        siteGeometry,
        { ...info, segment: info.finalSegment },
        analysis,
        "H-Group (level 3)",
      )!;
      const lines = comment.text.split("\n").length;
      const commentBox = {
        x: comment.left,
        y: comment.top,
        width: comment.width,
        height: lines * comment.fontSize * 1.4,
      };
      for (const other of [
        ...siteGeometry.bottomRow,
        ...Object.values(site.others),
      ]) {
        assert.ok(
          !overlap(commentBox, other),
          "the comment is clear of the site's buttons",
        );
      }

      for (const [i, button] of ours.entries()) {
        assert.ok(
          button.x >= 0 && button.x + size <= width,
          `button ${i} is on the canvas`,
        );
        for (const other of [
          ...site.controls,
          ...siteGeometry.bottomRow,
          ...Object.values(site.others),
        ]) {
          assert.ok(
            !overlap(button, other),
            `button ${i} covers ${JSON.stringify(other)}`,
          );
        }
        for (const other of ours.slice(i + 1)) {
          assert.ok(!overlap(button, other), `buttons ${i} touch each other`);
        }
      }
    });
  }
}

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
