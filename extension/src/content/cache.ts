import type { AnalysisResult } from "../protocol";

export interface CachedAnalysis {
  gameID: number;
  convention: string;
  label: string;
  result: AnalysisResult;
}

const INDEX_KEY = "analysis-index";
const KEEP = 40;

const recordKey = (gameID: number, convention: string) =>
  `analysis:${gameID}:${convention}`;
const latestKey = (gameID: number) => `analysis-latest:${gameID}`;

/** The analyses are kept in the browser, so opening a game again doesn't need the bot to go through it again. */
export async function loadAnalysis(
  gameID: number,
  convention: string,
): Promise<CachedAnalysis | null> {
  const key = recordKey(gameID, convention);
  const stored = await chrome.storage.local.get(key);
  return (stored[key] as CachedAnalysis | undefined) ?? null;
}

export async function loadLatestAnalysis(
  gameID: number,
): Promise<CachedAnalysis | null> {
  const key = latestKey(gameID);
  const stored = await chrome.storage.local.get(key);
  const convention = stored[key] as string | undefined;
  return convention === undefined ? null : loadAnalysis(gameID, convention);
}

export async function saveAnalysis(record: CachedAnalysis): Promise<void> {
  const key = recordKey(record.gameID, record.convention);
  const stored = await chrome.storage.local.get(INDEX_KEY);
  const index = ((stored[INDEX_KEY] as string[] | undefined) ?? []).filter(
    (k) => k !== key,
  );
  index.push(key);

  const dropped = index.splice(0, Math.max(0, index.length - KEEP));
  await chrome.storage.local.remove(dropped);
  await chrome.storage.local.set({
    [key]: record,
    [latestKey(record.gameID)]: record.convention,
    [INDEX_KEY]: index,
  });
}
