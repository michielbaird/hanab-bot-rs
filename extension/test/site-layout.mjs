// Where hanab.live draws the replay controls, from its own layout code (packages/client/src/game/ui/drawReplayArea.ts and
// replayView.ts), for a canvas of a given size. The numbers are fractions of the canvas width and height, as there.

const AREA = { x: 0.01, y: 0.49, w: 0.4, h: 0.27 };
const BAR = { y: 0.0425, h: 0.01 };
const SHUTTLE = 0.03;
const CONTROLS = { x: 0.05, y: 0.07, w: 0.06, h: 0.08, spacing: 0.02 };
const BOTTOM = { y: 0.17, h: 0.06, extra: 0.04 };

/**
 * The kinds of replay: "finished" (a finished game on its own), "game" (the replay inside a game that is still going on),
 * "leader" and "follower" (shared replays, as the one who leads it or not).
 *
 * @returns Absolute rectangles by element name, with the ones that are shown.
 */
export function getSiteLayout(winW, winH, kind) {
  const rect = (x, y, width, height) => ({ x, y, width, height });
  const area = rect(AREA.x * winW, AREA.y * winH, AREA.w * winW, AREA.h * winH);
  const inArea = (x, y, w, h) =>
    rect(area.x + x * winW, area.y + y * winH, w * winW, h * winH);

  const controls = [0, 1, 2, 3].map((i) =>
    inArea(
      CONTROLS.x + i * (CONTROLS.w + CONTROLS.spacing),
      CONTROLS.y,
      CONTROLS.w,
      CONTROLS.h,
    ),
  );
  const totalWidth = CONTROLS.w * 4 + CONTROLS.spacing * 3;
  const bottomWidth = CONTROLS.w * 2 + CONTROLS.spacing + BOTTOM.extra;
  const left = CONTROLS.x - BOTTOM.extra;
  const center = CONTROLS.x + (totalWidth - bottomWidth) / 2;
  const right = CONTROLS.x + CONTROLS.w * 2 + CONTROLS.spacing * 2;
  const bottom = (x) => inArea(x, BOTTOM.y, bottomWidth, BOTTOM.h);

  const shown = {
    finished: { enterHypoButton: center },
    game: { enterHypoButton: left, replayExitButton: right },
    leader: { pauseSharedTurnsButton: left, enterHypoButton: right },
    follower: { pauseSharedTurnsButton: center },
  }[kind];

  return {
    canvas: rect(0, 0, winW, winH),
    elements: {
      replayArea: area,
      replayBar: inArea(0, BAR.y, AREA.w, BAR.h),
      replayShuttle: rect(0, 0, SHUTTLE * winW, SHUTTLE * winH),
      replayBackFullButton: controls[0],
      replayForwardFullButton: controls[3],
    },
    // Only these are visible; the others exist but are hidden.
    bottomRow: Object.fromEntries(
      Object.entries(shown).map(([name, x]) => [name, bottom(x)]),
    ),
    controls,
  };
}
