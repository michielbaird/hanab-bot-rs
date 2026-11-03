import type { AnalysisResult, GameInfo, Geometry } from "../protocol";
import { commentsAt, getSegment, getSummary } from "./analysis";

export const MISTAKE_COLOR = "#ff4040";
export const SUGGESTION_COLOR = "#ffb000";
export const SUMMARY_COLOR = "#58a6ff";

const BUTTON_SIZE = 28;
const BUTTON_GAP = 4;

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
  text: string;
  mistake: boolean;
}

function getMarkerSize(bar: Geometry["bar"]) {
  return {
    width: Math.max(4, Math.round(bar.height * 0.9)),
    height: Math.max(7, Math.round(bar.height * 1.4)),
  };
}

/** Just above the shuttle, so that the shuttle is never covered. */
function getMarkerTop(geometry: Geometry): number {
  const { bar, shuttle } = geometry;
  return (
    bar.y + bar.height / 2 - shuttle.height / 2 - getMarkerSize(bar).height - 2
  );
}

/** The robot is above the right end of the markers, with the jump buttons to its left. */
export function getButtonPositions(geometry: Geometry) {
  const top = getMarkerTop(geometry) - (BUTTON_SIZE + BUTTON_GAP);
  const right = geometry.bar.x + geometry.bar.width - BUTTON_SIZE;
  const step = BUTTON_SIZE + BUTTON_GAP;
  return {
    robot: { left: right, top },
    next: { left: right - step, top },
    previous: { left: right - 2 * step, top },
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

  const at = (segment: number, color: string, title: string): Marker => ({
    left:
      bar.x +
      (sliderWidth / finalSegment) * segment +
      shuttle.width / 2 -
      width / 2,
    top,
    width,
    height,
    segment,
    color,
    title,
  });

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
      ),
    ),
    at(info.finalSegment, SUMMARY_COLOR, "The bot's summary of the game"),
  ];
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
    text = `${getSummary(analysis, convention)}\n(Unofficial; the bot is often wrong.)`;
  } else {
    return null;
  }

  const { replayArea } = geometry;
  return {
    left: replayArea.x,
    top: replayArea.y + replayArea.height * 0.9,
    width: replayArea.width,
    text,
    mistake: here.some((c) => c.mistake),
  };
}
