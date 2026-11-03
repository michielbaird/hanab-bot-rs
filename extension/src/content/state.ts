import type { AnalysisResult, PageState } from "../protocol";
import { getAnalyzable, getSupportKey } from "./analysis";

export interface Analysis {
  gameID: number;
  convention: string;
  result: AnalysisResult;
}

export type Progress =
  | { kind: "running"; text: string; done: number; total: number }
  | { kind: "error"; message: string };

export interface ContentState {
  page: PageState;
  /** Whether the bot can play a game, by its variant and options (see `getSupportKey`). */
  support: Readonly<Record<string, boolean>>;
  analysis: Analysis | null;
  panelOpen: boolean;
  progress: Progress | null;
}

export const initialState: ContentState = {
  page: { info: null, geometry: null },
  support: {},
  analysis: null,
  panelOpen: false,
  progress: null,
};

/** Whether the robot button is shown: a game the bot can analyze, on a page whose canvas is drawn. */
export function isVisible(state: ContentState): boolean {
  const { info, geometry } = state.page;
  const analyzable = getAnalyzable(info);
  return (
    analyzable !== null &&
    geometry !== null &&
    state.support[getSupportKey(analyzable)] === true
  );
}
