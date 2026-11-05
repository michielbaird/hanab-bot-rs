import type { AnalysisResult, PageState } from "../protocol";
import { DEFAULT_SETTINGS, type Settings } from "../settings";
import { getAnalyzable, getSupportKey, onlyMistakes } from "./analysis";

export interface Analysis {
  gameID: number;
  convention: string;
  result: AnalysisResult;
}

export type Progress =
  | { kind: "running"; text: string; done: number; total: number }
  | { kind: "error"; message: string };

export interface ContentState {
  settings: Settings;
  page: PageState;
  /** Whether the bot can play a game, by its variant and options (see `getSupportKey`). */
  support: Readonly<Record<string, boolean>>;
  analysis: Analysis | null;
  panelOpen: boolean;
  progress: Progress | null;
}

export const initialState: ContentState = {
  settings: DEFAULT_SETTINGS,
  page: { info: null, geometry: null },
  support: {},
  analysis: null,
  panelOpen: false,
  progress: null,
};

/** Whether the robot button is shown: a finished game in the database, on a page whose canvas is drawn. */
export function isVisible(state: ContentState): boolean {
  const { info, geometry } = state.page;
  return (
    state.settings.enabled && getAnalyzable(info) !== null && geometry !== null
  );
}

/** What the bot said, as far as the settings show it. */
export function getShownAnalysis(state: ContentState): AnalysisResult | null {
  if (state.analysis === null || !state.settings.enabled) {
    return null;
  }
  return state.settings.mistakesOnly
    ? onlyMistakes(state.analysis.result)
    : state.analysis.result;
}

/** Whether the bot can play the game on the page: `undefined` until the bot has been asked. */
export function getSupport(state: ContentState): boolean | undefined {
  const info = getAnalyzable(state.page.info);
  return info === null ? undefined : state.support[getSupportKey(info)];
}
