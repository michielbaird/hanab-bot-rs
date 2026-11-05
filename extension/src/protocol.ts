// The messages between the page bridge (`page.ts`), the content script (`content.ts`) and the worker (`worker.ts`).

export interface GameInfo {
  /** The game's id in the database, which "/export/<id>" takes. `null` for a game in progress. */
  databaseID: number | null;
  finished: boolean;
  variant: string;
  emptyClues: boolean;
  detrimentalCharacters: boolean;
  oneExtraCard: boolean;
  oneLessCard: boolean;
  segment: number;
  finalSegment: number;
  replayActive: boolean;
}

/** Where things are drawn on the page, in pixels from the top left of the page's viewport. */
export interface Geometry {
  bar: { x: number; y: number; width: number; height: number };
  shuttle: { width: number; height: number };
  replayArea: { x: number; y: number; width: number; height: number };
  /** The "Enter Hypothetical" button, beside which the extension's buttons go. */
  hypoButton: { x: number; y: number; width: number; height: number };
}

export interface PageState {
  info: GameInfo | null;
  geometry: Geometry | null;
}

export const PAGE_SOURCE = "hanab-analysis-page";
export const CONTENT_SOURCE = "hanab-analysis-content";

export interface PageMessage extends PageState {
  source: typeof PAGE_SOURCE;
  type: "state";
}

export interface GoToMessage {
  source: typeof CONTENT_SOURCE;
  type: "goto";
  segment: number;
}

export interface AnalysisComment {
  turn: number;
  player: number;
  mistake: boolean;
  text: string;
}

export interface AnalysisResult {
  players: string[];
  comments: AnalysisComment[];
}

export interface SupportsQuery {
  variant: string;
  emptyClues: boolean;
  detrimentalCharacters: boolean;
  oneExtraCard: boolean;
  oneLessCard: boolean;
}

export interface AnalyzeQuery {
  exportJson: string;
  variant: string;
  convention: string;
  endgameTimeoutMs: number;
}

/** Every request after `init` has an id, which its responses carry. */
export type WorkerRequest =
  | { type: "init"; wasm: ArrayBuffer }
  | ({ type: "supports"; id: number } & SupportsQuery)
  | ({ type: "analyze"; id: number } & AnalyzeQuery);

export type WorkerResponse = { id: number } & (
  | { type: "supports"; supported: boolean }
  | { type: "progress"; done: number; total: number }
  | { type: "result"; result: AnalysisResult }
  | { type: "error"; message: string }
);
