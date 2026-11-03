// The bot, in a Web Worker so that the page stays responsive. The content script starts it from a Blob (a page can't
// load a worker from an extension's URL) and sends the WebAssembly file as the first message.

import init, {
  analyze,
  init_variants,
  is_game_supported,
} from "hanab-bot-wasm";
import suitsJSON from "../build/data/suits.json";
import variantsJSON from "../build/data/variants.json";
import type { AnalysisResult, WorkerRequest, WorkerResponse } from "./protocol";

interface WorkerScope {
  postMessage: (message: WorkerResponse) => void;
  addEventListener: (
    type: "message",
    listener: (event: MessageEvent<WorkerRequest>) => void,
  ) => void;
}
const scope = globalThis as unknown as WorkerScope;

let loaded: Promise<void> | undefined;

function load(wasm: ArrayBuffer): Promise<void> {
  loaded ??= (async () => {
    await init({ module_or_path: wasm });
    init_variants(JSON.stringify(variantsJSON), JSON.stringify(suitsJSON));
  })();
  return loaded;
}

async function handle(request: WorkerRequest) {
  if (request.type === "init") {
    try {
      await load(request.wasm);
    } catch (error) {
      console.error("The bot could not be loaded:", error);
    }
    return;
  }

  try {
    if (loaded === undefined) {
      throw new Error("The bot was not started.");
    }
    await loaded;

    if (request.type === "supports") {
      scope.postMessage({
        id: request.id,
        type: "supports",
        supported: is_game_supported(
          request.variant,
          request.emptyClues,
          request.detrimentalCharacters,
          request.oneExtraCard,
          request.oneLessCard,
        ),
      });
      return;
    }

    const text = analyze(
      request.exportJson,
      request.variant,
      request.convention,
      request.endgameTimeoutMs,
      (done: number, total: number) => {
        scope.postMessage({ id: request.id, type: "progress", done, total });
      },
    );
    scope.postMessage({
      id: request.id,
      type: "result",
      result: JSON.parse(text) as AnalysisResult,
    });
  } catch (error) {
    scope.postMessage(
      request.type === "supports"
        ? { id: request.id, type: "supports", supported: false }
        : { id: request.id, type: "error", message: String(error) },
    );
  }
}

scope.addEventListener("message", (event) => {
  handle(event.data).catch((error: unknown) => {
    console.error(error);
  });
});
