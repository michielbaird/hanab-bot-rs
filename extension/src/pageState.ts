// What the extension reads from the site's replay state. The site changed it in November 2026 (`databaseID` became `gameId`,
// `replay.active` went away in favour of a `segment` that is absent outside a replay, and "nothing" became `undefined` instead
// of `null`), so both shapes are read.

export interface SiteReplay {
  active?: boolean;
  segment?: number | null;
  gameId?: number | null;
  databaseID?: number | null;
  hypothetical?: unknown;
  shared?: { useSharedSegments: boolean; amLeader: boolean } | null;
}

export function readReplay(replay: SiteReplay) {
  return {
    /** The game's id in the database, if it is saved there. */
    gameId: replay.gameId ?? replay.databaseID ?? null,
    segment: replay.segment ?? 0,
    active: replay.active ?? replay.segment != null,
    inHypothetical: replay.hypothetical != null,
    shared: replay.shared ?? null,
  };
}
