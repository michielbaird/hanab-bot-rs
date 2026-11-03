import type {
  AnalysisResult,
  AnalyzeQuery,
  SupportsQuery,
  WorkerRequest,
  WorkerResponse,
} from "../protocol";

export class Cancelled extends Error {
  constructor() {
    super("The analysis was cancelled.");
  }
}

export interface Engine {
  supports: (query: SupportsQuery) => Promise<boolean>;
  analyze: (
    query: AnalyzeQuery,
    onProgress: (done: number, total: number) => void,
  ) => Promise<AnalysisResult>;
  /** Ends whatever is running (the only way to stop the bot) and rejects the pending requests with `Cancelled`. */
  cancel: () => void;
}

interface Pending {
  resolve: (response: WorkerResponse) => void;
  reject: (error: Error) => void;
  onProgress: ((done: number, total: number) => void) | undefined;
}

type RequestBody =
  ({ type: "supports" } & SupportsQuery) | ({ type: "analyze" } & AnalyzeQuery);

/** The bot, which runs in a worker that is started when it is first needed and kept for the next request. */
export function createEngine(): Engine {
  let worker: Promise<Worker> | null = null;
  let nextId = 1;
  const pending = new Map<number, Pending>();

  function rejectAll(error: Error) {
    const entries = [...pending.values()];
    pending.clear();
    for (const entry of entries) {
      entry.reject(error);
    }
  }

  /** A page can't load a worker from an extension's URL, so it is started from a Blob that holds the worker's code. */
  async function createWorker(): Promise<Worker> {
    const [code, wasm] = await Promise.all([
      fetch(chrome.runtime.getURL("worker.js")).then(async (r) => r.text()),
      fetch(chrome.runtime.getURL("hanab_bot_wasm_bg.wasm")).then(async (r) =>
        r.arrayBuffer(),
      ),
    ]);

    const url = URL.createObjectURL(
      new Blob([code], { type: "text/javascript" }),
    );
    const created = new Worker(url, { type: "module" });
    created.addEventListener(
      "message",
      (event: MessageEvent<WorkerResponse>) => {
        const response = event.data;
        const entry = pending.get(response.id);
        if (entry === undefined) {
          return;
        }

        if (response.type === "progress") {
          entry.onProgress?.(response.done, response.total);
          return;
        }
        pending.delete(response.id);
        entry.resolve(response);
      },
    );
    created.addEventListener("error", () => {
      worker = null;
      rejectAll(new Error("The analysis stopped unexpectedly."));
    });

    const init: WorkerRequest = { type: "init", wasm };
    created.postMessage(init, [wasm]);
    return created;
  }

  async function request(
    body: RequestBody,
    onProgress?: (done: number, total: number) => void,
  ): Promise<WorkerResponse> {
    const id = nextId++;
    const response = new Promise<WorkerResponse>((resolve, reject) => {
      pending.set(id, { resolve, reject, onProgress });
    });

    worker ??= createWorker();
    try {
      const started = await worker;
      // Cancelled while the worker was starting.
      if (pending.has(id)) {
        started.postMessage({ ...body, id } satisfies WorkerRequest);
      }
    } catch (error) {
      worker = null;
      rejectAll(error instanceof Error ? error : new Error(String(error)));
    }
    return response;
  }

  return {
    async supports(query) {
      const response = await request({ type: "supports", ...query });
      return response.type === "supports" && response.supported;
    },

    async analyze(query, onProgress) {
      const response = await request({ type: "analyze", ...query }, onProgress);
      if (response.type === "result") {
        return response.result;
      }
      throw new Error(
        response.type === "error" ? response.message : "No result.",
      );
    },

    cancel() {
      const running = worker;
      worker = null;
      void running?.then((w) => {
        w.terminate();
      });
      rejectAll(new Cancelled());
    },
  };
}
