import type {
  AnalysisComment,
  AnalysisResult,
  GameInfo,
  SupportsQuery,
} from "../protocol";

/** The replay segment to show a comment at: the position just before the move, as the player saw it. */
export function getSegment(comment: AnalysisComment): number {
  return comment.turn - 1;
}

/** The stops of the jump buttons: the turns with a comment, and the end of the game (the summary). */
export function getStops(
  analysis: AnalysisResult,
  finalSegment: number,
): number[] {
  const segments = analysis.comments.map(getSegment);
  return [...new Set([...segments, finalSegment])].toSorted((a, b) => a - b);
}

export function commentsAt(
  analysis: AnalysisResult,
  segment: number,
): AnalysisComment[] {
  return analysis.comments.filter((c) => getSegment(c) === segment);
}

export function getSummary(
  analysis: AnalysisResult,
  convention: string,
): string {
  const total = analysis.comments.length;
  const mistakes = analysis.comments.filter((c) => c.mistake).length;
  const label = `Bot analysis (${convention})`;
  if (total === 0) {
    return `${label}: the bot would not have played any move differently.`;
  }

  const moves = total === 1 ? "1 move differs" : `${total} moves differ`;
  const clear =
    mistakes === 1 ? "1 clear mistake" : `${mistakes} clear mistakes`;
  return `${label}: ${moves} from what the bot would do, ${clear}.`;
}

export function getSupportQuery(info: GameInfo): SupportsQuery {
  return {
    variant: info.variant,
    emptyClues: info.emptyClues,
    detrimentalCharacters: info.detrimentalCharacters,
    oneExtraCard: info.oneExtraCard,
    oneLessCard: info.oneLessCard,
  };
}

export function getSupportKey(info: GameInfo): string {
  return JSON.stringify(getSupportQuery(info));
}

/** The finished games that are in the database are the ones that "/export/<id>" has. */
export function getAnalyzable(
  info: GameInfo | null,
): (GameInfo & { databaseID: number }) | null {
  if (
    info === null ||
    !info.finished ||
    info.databaseID === null ||
    info.databaseID <= 0
  ) {
    return null;
  }
  return { ...info, databaseID: info.databaseID };
}
