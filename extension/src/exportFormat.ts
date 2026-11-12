// The game that `/export/<id>` returns. Since November 2026 the site writes `playerNames`, turns named by `type`
// (`{type: "play", order}`) and `options.variantName`; the bot's WebAssembly package of before that reads `players`,
// numbered actions and `options.variant`. This turns the new shape into the old one (and leaves an old export alone), so the
// package works with both. A clue names its value as `color` or `rank` (it was `value`), and the options name the variant by
// `variantId` (it was `variantName`), which the page tells the name of.

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
  receiverIndex?: number;
  target?: number;
  value?: number;
  clue?: { type: string; value?: number; color?: number; rank?: number };
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
        // (The receiver of a clue was `target` until November 2026 too.)
        target: turn.receiverIndex ?? turn.target,
        value: turn.clue?.value ?? turn.clue?.color ?? turn.clue?.rank,
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

export function toOldExport(
  game: Record<string, unknown>,
  /** The name of the variant, for an export that gives only its number. */
  variant?: string,
): unknown {
  if (!("playerNames" in game)) {
    return game;
  }

  const { playerNames, actions, options, gameId, ...rest } = game as {
    playerNames: unknown;
    actions?: Turn[];
    options?: Record<string, unknown>;
    gameId?: number;
  };
  const { variantName, variantId: _variantId, ...flags } = options ?? {};
  return {
    ...rest,
    players: playerNames,
    actions: (actions ?? []).map(oldTurn),
    options: {
      variant: variantName ?? variant ?? "No Variant",
      ...flags,
    },
    id: gameId,
  };
}

/** The export as text, in the shape that the bot's package reads. */
export function normalizeExport(text: string, variant?: string): string {
  return JSON.stringify(
    toOldExport(JSON.parse(text) as Record<string, unknown>, variant),
  );
}
