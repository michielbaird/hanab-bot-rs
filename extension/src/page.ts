// Runs in the page's own context (`"world": "MAIN"`): the isolated content script can't see "window.globals", where the
// site keeps the game's state. Reads what the extension needs and moves the replay. Never throws into the site.

import { readReplay, type SiteReplay } from "./pageState";
import {
  CONTENT_SOURCE,
  PAGE_SOURCE,
  type GoToMessage,
  type PageMessage,
  type PageState,
  type Rect,
} from "./protocol";

interface KonvaNode {
  isVisible: () => boolean;
  /** Null once the node is no longer on a stage, which is what the site's destroy() leaves. */
  getStage: () => unknown;
  getAbsolutePosition: () => { x: number; y: number };
  width: () => number;
  height: () => number;
}

interface GameState {
  finished: boolean;
  replay: SiteReplay;
  ongoingGame: { turn: { segment?: number | null } };
  metadata: {
    options: {
      variantName: string;
      emptyClues: boolean;
      detrimentalCharacters: boolean;
      oneExtraCard: boolean;
      oneLessCard: boolean;
    };
  };
}

interface Store {
  getState: () => GameState;
  dispatch: (action: unknown) => void;
}

interface PageGlobals {
  store: Store | null;
  stage: { container: () => HTMLElement };
  elements: {
    replayBar: KonvaNode | null;
    replayShuttle: KonvaNode | null;
    replayArea: KonvaNode | null;
    replayBackFullButton: KonvaNode | null;
    replayForwardFullButton: KonvaNode | null;
    enterHypoButton: KonvaNode | null;
    pauseSharedTurnsButton: KonvaNode | null;
    useSharedTurnsButton: KonvaNode | null;
    replayExitButton: KonvaNode | null;
  };
}

const POLL_MS = 300;

function getGlobals(): PageGlobals | undefined {
  return (window as unknown as { globals?: PageGlobals }).globals;
}

function read(): PageState {
  try {
    const pageGlobals = getGlobals();
    const store = pageGlobals?.store;
    if (pageGlobals === undefined || !store) {
      return { info: null, geometry: null };
    }

    const state = store.getState();
    const { options } = state.metadata;
    const replay = readReplay(state.replay);
    const info = {
      databaseID: replay.gameId,
      finished: state.finished,
      variant: options.variantName,
      emptyClues: options.emptyClues,
      detrimentalCharacters: options.detrimentalCharacters,
      oneExtraCard: options.oneExtraCard,
      oneLessCard: options.oneLessCard,
      segment: replay.segment,
      finalSegment: state.ongoingGame.turn.segment ?? 0,
      replayActive: replay.active,
    };

    const {
      replayBar,
      replayShuttle,
      replayArea,
      replayBackFullButton,
      replayForwardFullButton,
    } = pageGlobals.elements;
    if (
      !replayBar ||
      !replayShuttle ||
      !replayArea ||
      // Hidden in a hypothetical, where the hypothetical's buttons take its place.
      !replayArea.isVisible() ||
      // Leaving the game (back, or to the lobby) destroys the canvas but leaves the site's state as it was.
      !replayArea.getStage() ||
      !pageGlobals.stage.container().isConnected ||
      !replayBackFullButton ||
      !replayForwardFullButton
    ) {
      return { info, geometry: null };
    }

    // The canvas positions are from the top left of the canvas's container.
    const origin = pageGlobals.stage.container().getBoundingClientRect();
    const place = (node: KonvaNode) => {
      const position = node.getAbsolutePosition();
      return {
        x: Math.round(origin.left + position.x),
        y: Math.round(origin.top + position.y),
        width: Math.round(node.width()),
        height: Math.round(node.height()),
      };
    };
    const spanning = (first: Rect, last: Rect): Rect => ({
      ...first,
      width: last.x + last.width - first.x,
    });
    return {
      info,
      geometry: {
        bar: place(replayBar),
        shuttle: {
          width: Math.round(replayShuttle.width()),
          height: Math.round(replayShuttle.height()),
        },
        replayArea: place(replayArea),
        controls: spanning(
          place(replayBackFullButton),
          place(replayForwardFullButton),
        ),
        bottomRow: [
          pageGlobals.elements.enterHypoButton,
          pageGlobals.elements.pauseSharedTurnsButton,
          pageGlobals.elements.useSharedTurnsButton,
          pageGlobals.elements.replayExitButton,
        ]
          .filter((node) => node?.isVisible() === true)
          .map((node) => place(node!)),
      },
    };
  } catch {
    return { info: null, geometry: null };
  }
}

let last = "";

function report() {
  const state = read();
  const text = JSON.stringify(state);
  if (text === last) {
    return;
  }
  last = text;

  const message: PageMessage = { source: PAGE_SOURCE, type: "state", ...state };
  window.postMessage(message, window.location.origin);
}

/** The same moves as the site's own "goToSegment()" (in its "replay.ts"). */
function goTo(segment: number) {
  try {
    const store = getGlobals()?.store;
    if (!store) {
      return;
    }

    const state = store.getState();
    const replay = readReplay(state.replay);
    // The site doesn't move the replay during a hypothetical either.
    if (replay.inHypothetical) {
      return;
    }

    const finalSegment = state.ongoingGame.turn.segment ?? 0;
    const target = Math.max(0, Math.min(segment, finalSegment));
    if (!replay.active) {
      store.dispatch({ type: "replayEnter", segment: target });
    }

    const { shared } = replay;
    if (shared !== null && shared.useSharedSegments && !shared.amLeader) {
      store.dispatch({
        type: "replayUseSharedSegments",
        useSharedSegments: false,
      });
    }

    store.dispatch({ type: "replaySegment", segment: target });
    if (shared !== null && shared.amLeader && shared.useSharedSegments) {
      store.dispatch({ type: "replaySharedSegment", segment: target });
    }
  } catch {
    // Not a page that we understand.
  }
}

window.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (event.source !== window) {
    return;
  }
  const message = event.data as Partial<GoToMessage> | null;
  if (
    message?.source === CONTENT_SOURCE &&
    message.type === "goto" &&
    typeof message.segment === "number"
  ) {
    goTo(message.segment);
  }
});

setInterval(report, POLL_MS);
report();
