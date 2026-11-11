// The game that `/export/<id>` returns. Since November 2026 the site writes `playerNames`, turns named by `type`
// (`{type: "play", order}`) and `options.variantName`; the bot's WebAssembly package of before that reads `players`,
// numbered actions and `options.variant`. This turns the new shape into the old one (and leaves an old export alone), so the
// package works with both.

const END_CONDITIONS: Record<string, number> = {
  normal: 1,
  strikeout: 2,
  timeout: 3,
  terminatedByPlayer: 4,
  speedrunFail: 5,
  idleTimeout: 6,
  characterSoftlock: 7,
  allOrNothingFail: 8,
  allOrNothingSoftlock: 9,
  terminatedByVote: 10,
};

interface Turn {
  type: string | number;
  order?: number;
  target?: number;
  value?: number;
  clue?: { type: string; value: number };
  endCondition?: string | number;
  playerIndex?: number;
}

function oldTurn(turn: Turn): unknown {
  if (typeof turn.type === "number") {
    return turn;
  }
  switch (turn.type) {
    case "play":
      return { type: 0, target: turn.order, value: 0 };
    case "discard":
      return { type: 1, target: turn.order, value: 0 };
    case "clue":
      return {
        type: turn.clue?.type === "rank" ? 3 : 2,
        target: turn.target,
        value: turn.clue?.value,
      };
    case "gameOver":
      return {
        type: 4,
        target: turn.playerIndex ?? 0,
        value:
          typeof turn.endCondition === "string"
            ? (END_CONDITIONS[turn.endCondition] ?? 0)
            : turn.endCondition,
      };
    default:
      throw new Error(`Unknown turn type ${turn.type}`);
  }
}

export function toOldExport(game: Record<string, unknown>): unknown {
  if (!("playerNames" in game)) {
    return game;
  }

  const { playerNames, actions, options, gameId, ...rest } = game as {
    playerNames: unknown;
    actions?: Turn[];
    options?: Record<string, unknown>;
    gameId?: number;
  };
  const { variantName, ...flags } = options ?? {};
  return {
    ...rest,
    players: playerNames,
    actions: (actions ?? []).map(oldTurn),
    options: { variant: variantName ?? "No Variant", ...flags },
    id: gameId,
  };
}

/** The export as text, in the shape that the bot's package reads. */
export function normalizeExport(text: string): string {
  return JSON.stringify(
    toOldExport(JSON.parse(text) as Record<string, unknown>),
  );
}
