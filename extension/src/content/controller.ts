import { saveSettings } from "../settings";
import {
  getAnalyzable,
  getStops,
  getSupportKey,
  getSupportQuery,
} from "./analysis";
import { loadAnalysis, loadLatestAnalysis, saveAnalysis } from "./cache";
import { Cancelled, type Engine } from "./engine";
import { getShownAnalysis, isVisible, type ContentState } from "./state";
import type { Store } from "./store";

const ERROR_SHOWN_MS = 8000;
const ENDGAME_TIMEOUT_MS = 2000;
const ENDGAME_TIMEOUT_SLOW_MS = 6000;

export interface Controller {
  togglePanel: () => void;
  closePanel: () => void;
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
  /** The game that a saved analysis was looked for, so that it is only looked for once. */
  let restoredFor: number | null = null;

  function showError(message: string) {
    const progress = { kind: "error", message } as const;
    store.update({ progress });
    setTimeout(() => {
      if (store.get().progress === progress) {
        store.update({ progress: null });
      }
    }, ERROR_SHOWN_MS);
  }

  /** Starts the bot (it is not loaded before this is needed), and the answer arrives in the state. */
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

  /** An analysis made earlier of the game on the page comes back without the bot. */
  function restore(gameID: number) {
    loadLatestAnalysis(gameID)
      .then((cached) => {
        const { analysis, page } = store.get();
        if (
          cached !== null &&
          analysis === null &&
          getAnalyzable(page.info)?.databaseID === gameID
        ) {
          store.update({
            analysis: {
              gameID,
              convention: cached.label,
              result: cached.result,
            },
          });
        }
      })
      .catch(() => undefined);
  }

  async function analyze(convention: string, conventionLabel: string) {
    const info = getAnalyzable(store.get().page.info);
    if (info === null) {
      showError("This game can not be analyzed.");
      return;
    }

    saveSettings({ convention }).catch(() => undefined);
    store.update({ panelOpen: false });

    const cached = await loadAnalysis(info.databaseID, convention).catch(
      () => null,
    );
    if (cached !== null) {
      store.update({
        analysis: {
          gameID: info.databaseID,
          convention: conventionLabel,
          result: cached.result,
        },
      });
      return;
    }

    const running = (text: string, done = 0, total = 0) => {
      store.update({ progress: { kind: "running", text, done, total } });
    };
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
          // The browser is slower than a native bot; a search that runs out of time says nothing about the move.
          endgameTimeoutMs: store.get().settings.slowDevice
            ? ENDGAME_TIMEOUT_SLOW_MS
            : ENDGAME_TIMEOUT_MS,
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
      saveAnalysis({
        gameID: info.databaseID,
        convention,
        label: conventionLabel,
        result,
      }).catch(() => undefined);
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

    closePanel() {
      store.update({ panelOpen: false });
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
      const state = store.get();
      const analysis = getShownAnalysis(state);
      if (state.page.info === null || analysis === null) {
        return;
      }

      const { segment, finalSegment } = state.page.info;
      const stops = getStops(analysis, finalSegment).filter((s) =>
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
    if (info === null) {
      restoredFor = null;
    }

    if (
      state.analysis !== null &&
      (info === null || info.databaseID !== state.analysis.gameID)
    ) {
      restoredFor = null;
      store.update({ analysis: null });
      return;
    }

    if (state.progress !== null && !isVisible(state)) {
      // Left the game: a running analysis is of no use any more.
      engine.cancel();
      store.update({ progress: null });
    }

    if (state.panelOpen) {
      askSupport(state);
      if (!isVisible(state)) {
        store.update({ panelOpen: false });
      }
    }

    if (
      info !== null &&
      state.analysis === null &&
      state.settings.enabled &&
      restoredFor !== info.databaseID
    ) {
      restoredFor = info.databaseID;
      restore(info.databaseID);
    }
  });

  return controller;
}
