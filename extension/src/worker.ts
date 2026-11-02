// The bot's game analysis, in a Web Worker so that the page stays responsive while it thinks (an
// analysis takes many seconds). The content script starts it from a Blob (a page can't load a worker
// from an extension's URL) and sends the WebAssembly file as the first message.

import init, {
  analyze,
  init_variants,
  is_game_supported,
} from "hanab-bot-wasm";
import suitsJSON from "../build/data/suits.json";
import variantsJSON from "../build/data/variants.json";
import type { WorkerRequest, WorkerResponse } from "./protocol";

/** The parts of the worker's global scope that are used (this is compiled for the DOM). */
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
        scope.postMessage({ type: "progress", done, total });
      },
    );
    scope.postMessage({
      type: "result",
      result: JSON.parse(text) as Extract<
        WorkerResponse,
        { type: "result" }
      >["result"],
    });
  } catch (error) {
    // A question about support that fails means that the bot can't be used.
    scope.postMessage(
      request.type === "supports"
        ? { type: "supports", supported: false }
        : { type: "error", message: String(error) },
    );
  }
}

scope.addEventListener("message", (event) => {
  handle(event.data).catch((error: unknown) => {
    console.error(error);
  });
});
