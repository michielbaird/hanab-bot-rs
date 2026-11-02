// The page bridge. It runs in the page's own JavaScript context (a content script with
// `"world": "MAIN"`), because the game keeps its state in objects that the extension's isolated
// content script can't see: the site sets "window.globals" (the game's UI state, with the Redux
// store and the Konva objects of the canvas). This script reads what the extension needs, tells the
// content script when it changes, and moves the replay when asked. It does nothing else, and it
// never throws into the site.

import {
  CONTENT_SOURCE,
  PAGE_SOURCE,
  type GoToMessage,
  type PageMessage,
  type PageState,
} from "./protocol";

/** The parts of a Konva node that are used. */
interface KonvaNode {
  getAbsolutePosition: () => { x: number; y: number };
  width: () => number;
  height: () => number;
}

interface GameState {
  finished: boolean;
  replay: {
    active: boolean;
    segment: number;
    databaseID: number | null;
    hypothetical: unknown;
    shared: { useSharedSegments: boolean; amLeader: boolean } | null;
  };
  ongoingGame: { turn: { segment: number | null } };
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

/** The parts of the site's "window.globals" that are used. */
interface PageGlobals {
  store: Store | null;
  stage: { container: () => HTMLElement };
  elements: {
    replayBar: KonvaNode | null;
    replayShuttle: KonvaNode | null;
    replayArea: KonvaNode | null;
  };
}

const POLL_MS = 300;

function getGlobals(): PageGlobals | undefined {
  return (window as unknown as { globals?: PageGlobals }).globals;
}

/** What is on the page now. Anything unexpected reads as "no game". */
function read(): PageState {
  try {
    const pageGlobals = getGlobals();
    const store = pageGlobals?.store;
    if (pageGlobals === undefined || !store) {
      return { info: null, geometry: null };
    }

    const state = store.getState();
    const { options } = state.metadata;
    const info = {
      databaseID: state.replay.databaseID,
      finished: state.finished,
      variant: options.variantName,
      emptyClues: options.emptyClues,
      detrimentalCharacters: options.detrimentalCharacters,
      oneExtraCard: options.oneExtraCard,
      oneLessCard: options.oneLessCard,
      segment: state.replay.segment,
      finalSegment: state.ongoingGame.turn.segment ?? 0,
      replayActive: state.replay.active,
    };

    const { replayBar, replayShuttle, replayArea } = pageGlobals.elements;
    if (!replayBar || !replayShuttle || !replayArea) {
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
    return {
      info,
      geometry: {
        bar: place(replayBar),
        shuttle: {
          width: Math.round(replayShuttle.width()),
          height: Math.round(replayShuttle.height()),
        },
        replayArea: place(replayArea),
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
    // The site doesn't move the replay during a hypothetical either.
    if (state.replay.hypothetical !== null) {
      return;
    }

    const finalSegment = state.ongoingGame.turn.segment ?? 0;
    const target = Math.max(0, Math.min(segment, finalSegment));
    if (!state.replay.active) {
      store.dispatch({ type: "replayEnter", segment: target });
    }

    const { shared } = state.replay;
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
    message?.source === CONTENT_SOURCE
    && message.type === "goto"
    && typeof message.segment === "number"
  ) {
    goTo(message.segment);
  }
});

setInterval(report, POLL_MS);
report();
