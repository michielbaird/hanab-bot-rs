import type { AnalysisResult, GameInfo, Geometry } from "../protocol";

export const info: GameInfo = {
  databaseID: 7,
  finished: true,
  variant: "No Variant",
  emptyClues: false,
  detrimentalCharacters: false,
  oneExtraCard: false,
  oneLessCard: false,
  segment: 0,
  finalSegment: 72,
  replayActive: true,
};

export const geometry: Geometry = {
  bar: { x: 100, y: 300, width: 500, height: 8 },
  shuttle: { width: 24, height: 24 },
  replayArea: { x: 60, y: 260, width: 600, height: 220 },
  controls: { x: 110, y: 340, width: 240, height: 40 },
  bottomRow: [{ x: 250, y: 420, width: 220, height: 40 }],
};

export const analysis: AnalysisResult = {
  players: ["Alice", "Bob"],
  comments: [
    { turn: 9, player: 1, mistake: true, text: "mistake! prefer (a) over (b)" },
    { turn: 4, player: 0, mistake: false, text: "suggest (a) over (b)" },
    { turn: 20, player: 0, mistake: false, text: "suggest (c) over (d)" },
  ],
};
