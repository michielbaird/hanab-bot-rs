import type { AnalysisResult, GameInfo, Geometry, Rect } from "../protocol";
import { commentsAt, getSegment, getSummary } from "./analysis";

// Colours that stay apart with the common kinds of colour blindness (the Okabe-Ito palette); the heights differ too.
export const MISTAKE_COLOR = "#d55e00";
export const SUGGESTION_COLOR = "#f0e442";
export const SUMMARY_COLOR = "#56b4e9";

const BUTTON_SIZE = 28;
const BUTTON_GAP = 2;

export interface Point {
  left: number;
  top: number;
}

export interface Marker extends Point {
  width: number;
  height: number;
  segment: number;
  color: string;
  title: string;
}

export interface CommentBox extends Point {
  width: number;
  fontSize: number;
  text: string;
  mistake: boolean;
}

function getMarkerSize(bar: Geometry["bar"]) {
  return {
    width: Math.max(4, Math.round(bar.height * 0.9)),
    height: Math.max(6, Math.round(bar.height * 1.1)),
  };
}

/** Just above the shuttle, so that the shuttle is never covered. */
function getMarkerTop(geometry: Geometry): number {
  const { bar, shuttle } = geometry;
  return (
    bar.y + bar.height / 2 - shuttle.height / 2 - getMarkerSize(bar).height
  );
}

/**
 * Where the robot can go in the row under the controls: the widest gap between two buttons (a shared replay has the
 * site's own robot spot between them), or with one button, the wider side of it.
 */
function getFreeSpot(geometry: Geometry): Rect | null {
  const { replayArea } = geometry;
  const buttons = geometry.bottomRow.toSorted((a, b) => a.x - b.x);
  const [first] = buttons;
  const last = buttons.at(-1);
  if (first === undefined || last === undefined) {
    return null;
  }
  const spot = (from: number, to: number): Rect => ({
    x: from,
    y: first.y,
    width: to - from,
    height: first.height,
  });
  const gaps = buttons
    .slice(1)
    .map((button, i) => spot(buttons[i]!.x + buttons[i]!.width, button.x));
  if (gaps.length === 0) {
    gaps.push(
      spot(replayArea.x, first.x),
      spot(first.x + first.width, replayArea.x + replayArea.width),
    );
  }
  return gaps.reduce((best, gap) => (gap.width > best.width ? gap : best));
}

/**
 * The robot goes in the widest free space in the row under the controls (beside "Enter Hypothetical", or between the
 * buttons of a shared replay), the previous and next buttons to the left of the controls.
 */
export function getButtonPositions(geometry: Geometry) {
  const { controls } = geometry;
  const free = getFreeSpot(geometry);
  const robot =
    free !== null && free.width >= BUTTON_SIZE
      ? {
          left: free.x + (free.width - BUTTON_SIZE) / 2,
          top: free.y + (free.height - BUTTON_SIZE) / 2,
        }
      : {
          left: controls.x - 3 * BUTTON_SIZE - 3 * BUTTON_GAP,
          top: controls.y + (controls.height - BUTTON_SIZE) / 2,
        };
  const top = controls.y + (controls.height - BUTTON_SIZE) / 2;
  const step = BUTTON_SIZE + BUTTON_GAP;
  return {
    robot,
    next: { left: controls.x - step, top },
    previous: { left: controls.x - 2 * step, top },
  };
}

export function getMarkers(
  geometry: Geometry,
  info: GameInfo,
  analysis: AnalysisResult,
): Marker[] {
  const { bar, shuttle } = geometry;
  const finalSegment = Math.max(info.finalSegment, 1);
  const sliderWidth = bar.width - shuttle.width;
  const { width, height } = getMarkerSize(bar);
  const top = getMarkerTop(geometry);

  const at = (
    segment: number,
    color: string,
    title: string,
    shorter = false,
  ): Marker => {
    const markerHeight = shorter ? Math.round(height * 0.7) : height;
    return {
      left:
        bar.x +
        (sliderWidth / finalSegment) * segment +
        shuttle.width / 2 -
        width / 2,
      top: top + height - markerHeight,
      width,
      height: markerHeight,
      segment,
      color,
      title,
    };
  };

  // Mistakes last, so that they are on top if a suggestion is on the same turn.
  const ordered = analysis.comments.toSorted(
    (a, b) => Number(a.mistake) - Number(b.mistake),
  );
  return [
    ...ordered.map((c) =>
      at(
        getSegment(c),
        c.mistake ? MISTAKE_COLOR : SUGGESTION_COLOR,
        `Turn ${c.turn}: ${c.text}`,
        !c.mistake,
      ),
    ),
    at(info.finalSegment, SUMMARY_COLOR, "The bot's summary of the game"),
  ];
}

const COMMENT_FONT = { max: 13, min: 9, charWidth: 0.55 };

/** The site draws smaller when the window is small, so the text shrinks to stay on one line instead of running into what is below. */
function getCommentFontSize(text: string, width: number): number {
  const longest = Math.max(...text.split("\n").map((line) => line.length));
  const fitting = width / (longest * COMMENT_FONT.charWidth);
  return Math.round(
    Math.min(COMMENT_FONT.max, Math.max(COMMENT_FONT.min, fitting)),
  );
}

/** What to say under the replay controls: the comments on this turn, or the summary at the end. */
export function getCommentBox(
  geometry: Geometry,
  info: GameInfo,
  analysis: AnalysisResult,
  convention: string,
): CommentBox | null {
  const here = commentsAt(analysis, info.segment);
  let text: string;
  if (here.length > 0) {
    text = here
      .map((c) => `${analysis.players[c.player] ?? "?"}: ${c.text}`)
      .join("\n");
  } else if (info.segment === info.finalSegment) {
    text = getSummary(analysis, convention);
  } else {
    return null;
  }

  const { replayArea } = geometry;
  return {
    left: replayArea.x,
    top: replayArea.y + replayArea.height * 0.9,
    width: replayArea.width,
    fontSize: getCommentFontSize(text, replayArea.width),
    text,
    mistake: here.some((c) => c.mistake),
  };
}
