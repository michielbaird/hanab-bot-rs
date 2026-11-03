import {
  getAnalyzable,
  getStops,
  getSupportKey,
  getSupportQuery,
} from "./analysis";
import { Cancelled, type Engine } from "./engine";
import { isVisible, type ContentState } from "./state";
import type { Store } from "./store";

const ERROR_SHOWN_MS = 8000;

export interface Controller {
  togglePanel: () => void;
  startAnalysis: (convention: string, conventionLabel: string) => void;
  cancelAnalysis: () => void;
  dismissProgress: () => void;
  jump: (direction: -1 | 1) => void;
  goTo: (segment: number) => void;
}

/** What the user can do, and what follows from the state changing (which game it is, what the bot can play). */
export function createController(
  store: Store<ContentState>,
  engine: Engine,
  goTo: (segment: number) => void,
): Controller {
  const askedSupport = new Set<string>();

  function showError(message: string) {
    const progress = { kind: "error", message } as const;
    store.update({ progress });
    setTimeout(() => {
      if (store.get().progress === progress) {
        store.update({ progress: null });
      }
    }, ERROR_SHOWN_MS);
  }

  /** The answer is only known once the bot has replied; until then the game counts as unsupported. */
  function askSupport(state: ContentState) {
    const info = getAnalyzable(state.page.info);
    if (info === null) {
      return;
    }

    const key = getSupportKey(info);
    if (key in state.support || askedSupport.has(key)) {
      return;
    }

    askedSupport.add(key);
    const answer = (supported: boolean) => {
      store.update((s) => ({ support: { ...s.support, [key]: supported } }));
    };
    engine
      .supports(getSupportQuery(info))
      .then(answer)
      .catch((error: unknown) => {
        if (!(error instanceof Cancelled)) {
          answer(false);
        }
      })
      .finally(() => {
        askedSupport.delete(key);
      });
  }

  async function analyze(convention: string, conventionLabel: string) {
    const info = getAnalyzable(store.get().page.info);
    if (info === null) {
      showError("This game can not be analyzed.");
      return;
    }

    const running = (text: string, done = 0, total = 0) => {
      store.update({ progress: { kind: "running", text, done, total } });
    };
    store.update({ panelOpen: false });
    running("Loading the game...");

    try {
      const response = await fetch(`/export/${info.databaseID}`);
      if (!response.ok) {
        throw new Error(`The server could not export game ${info.databaseID}.`);
      }
      const exportJson = await response.text();

      running("Bot analysis: starting...");
      const result = await engine.analyze(
        {
          exportJson,
          variant: info.variant,
          convention,
          // The browser is slower than a native bot; a search that runs out of time reads as a mistake.
          endgameTimeoutMs: 2000,
        },
        (done, total) => {
          running(`Bot analysis: move ${done} of ${total}`, done, total);
        },
      );
      store.update({
        analysis: {
          gameID: info.databaseID,
          convention: conventionLabel,
          result,
        },
        progress: null,
      });
    } catch (error) {
      // A cancelled analysis has already dismissed its progress bar.
      if (!(error instanceof Cancelled)) {
        showError(error instanceof Error ? error.message : String(error));
      }
    }
  }

  const controller: Controller = {
    togglePanel() {
      store.update((s) => ({ panelOpen: !s.panelOpen }));
    },

    startAnalysis(convention, conventionLabel) {
      analyze(convention, conventionLabel).catch((error: unknown) => {
        showError(String(error));
      });
    },

    cancelAnalysis() {
      engine.cancel();
      store.update({ progress: null });
    },

    dismissProgress() {
      store.update({ progress: null });
    },

    jump(direction) {
      const { page, analysis } = store.get();
      if (page.info === null || analysis === null) {
        return;
      }

      const { segment, finalSegment } = page.info;
      const stops = getStops(analysis.result, finalSegment).filter((s) =>
        direction > 0 ? s > segment : s < segment,
      );
      const target = direction > 0 ? stops[0] : stops.at(-1);
      if (target !== undefined) {
        goTo(target);
      }
    },

    goTo,
  };

  store.subscribe((state) => {
    const info = getAnalyzable(state.page.info);
    if (
      state.analysis !== null &&
      (info === null || info.databaseID !== state.analysis.gameID)
    ) {
      store.update({ analysis: null });
      return;
    }

    askSupport(state);
    if (state.panelOpen && !isVisible(state)) {
      store.update({ panelOpen: false });
    }
  });

  return controller;
}
