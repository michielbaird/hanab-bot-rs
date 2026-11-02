// What the extension's scripts say to each other. The page bridge ("page.ts") runs in the page's own
// JavaScript context, where it can read the game's state; the content script ("content.ts") has the
// extension's privileges. They talk with "window.postMessage".

/** What the bridge reads from the game. */
export interface GameInfo {
  /** The game's id in the database, which "/export/<id>" takes. `null` for a game in progress. */
  databaseID: number | null;
  finished: boolean;
  variant: string;
  emptyClues: boolean;
  detrimentalCharacters: boolean;
  oneExtraCard: boolean;
  oneLessCard: boolean;
  /** The replay segment being shown. */
  segment: number;
  /** The last replay segment. */
  finalSegment: number;
  replayActive: boolean;
}

/** Where things are drawn on the page, in pixels from the top left of the page's viewport. */
export interface Geometry {
  /** The thin bar that the replay shuttle slides on. */
  bar: { x: number; y: number; width: number; height: number };
  shuttle: { width: number; height: number };
  /** The group of replay controls (the bar, the buttons, ...). */
  replayArea: { x: number; y: number; width: number; height: number };
}

export interface PageState {
  info: GameInfo | null;
  geometry: Geometry | null;
}

export const PAGE_SOURCE = "hanab-analysis-page";
export const CONTENT_SOURCE = "hanab-analysis-content";

/** Page -> content script. */
export interface PageMessage extends PageState {
  source: typeof PAGE_SOURCE;
  type: "state";
}

/** Content script -> page. */
export interface GoToMessage {
  source: typeof CONTENT_SOURCE;
  type: "goto";
  segment: number;
}

/** The comments the bot makes on a game (the output of the wasm module's "analyze"). */
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

/** Content script -> worker. */
export type WorkerRequest =
  | { type: "init"; wasm: ArrayBuffer }
  | {
      type: "supports";
      variant: string;
      emptyClues: boolean;
      detrimentalCharacters: boolean;
      oneExtraCard: boolean;
      oneLessCard: boolean;
    }
  | {
      type: "analyze";
      exportJson: string;
      variant: string;
      convention: string;
      endgameTimeoutMs: number;
    };

/** Worker -> content script. */
export type WorkerResponse =
  | { type: "supports"; supported: boolean }
  | { type: "progress"; done: number; total: number }
  | { type: "result"; result: AnalysisResult }
  | { type: "error"; message: string };
