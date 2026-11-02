// The content script: the extension's part that lives on hanab.live. It shows a robot button for
// finished games, lets you pick a convention, has the bot go through the game (in a worker, see
// "worker.ts") and marks the moves it would have played differently on the replay bar. What it
// knows about the game and the canvas comes from the page bridge ("page.ts").
//
// The extension is unofficial. Its disclaimer is shown wherever the bot's opinions are.

import {
  CONTENT_SOURCE,
  PAGE_SOURCE,
  type AnalysisComment,
  type AnalysisResult,
  type GameInfo,
  type GoToMessage,
  type PageMessage,
  type PageState,
  type WorkerRequest,
  type WorkerResponse,
} from "./protocol";

const DISCLAIMER =
  "Unofficial. Not made or endorsed by hanab.live or the H-Group. The bot is often wrong, "
  + "especially at higher H-Group levels: treat what it says as a second opinion, not as advice.";

const H_GROUP_LEVELS = Array.from({ length: 12 }, (_, i) => i + 1);
const CONVENTIONS: ReadonlyArray<readonly [string, string]> = [
  ...H_GROUP_LEVELS.map((level): readonly [string, string] => [
    `HGroup${level}`,
    `H-Group (level ${level})`,
  ]),
  ["Reactor", "Reactor"],
  ["RefSieve", "Referential Sieve"],
];
const CONVENTION_KEY = "hanab-analysis-convention";

const MISTAKE_COLOR = "#ff4040";
const SUGGESTION_COLOR = "#ffb000";
const SUMMARY_COLOR = "#58a6ff";

// ------
// State
// ------

let page: PageState = { info: null, geometry: null };
let result: AnalysisResult | null = null;
let resultGameID: number | null = null;
let resultConvention = "";
/** Whether the bot can follow a game, by its variant and options. */
const supportCache = new Map<string, boolean>();

// ---
// UI
// ---

const STYLE = `
  :host { all: initial; }
  * { box-sizing: border-box; font-family: sans-serif; }
  button { cursor: pointer; }
  .robot, .nav {
    position: fixed; z-index: 2147483000; width: 28px; height: 28px; padding: 0;
    border: 1px solid #000; border-radius: 6px; background: rgba(0,0,0,0.75); color: #fff; font-size: 15px;
  }
  .nav { font-size: 11px; font-weight: bold; }
  .nav:disabled { opacity: 0.35; cursor: default; }
  .panel, .progress {
    position: fixed; z-index: 2147483001; padding: 10px 14px; border-radius: 6px;
    background: rgba(0,0,0,0.92); color: #fff; font-size: 13px; line-height: 1.4;
  }
  .panel { width: 300px; }
  .panel select, .panel button { font-size: 13px; margin-top: 6px; }
  .panel .small, .progress .small { font-size: 11px; color: #bbb; margin-top: 8px; }
  .progress { top: 50%; left: 50%; transform: translate(-50%, -50%); text-align: center; }
  .progress progress { width: 260px; display: block; margin: 8px auto; }
  .progress .error { color: #ff8080; }
  .tick { position: fixed; z-index: 2147482999; border-radius: 2px; cursor: pointer; }
  .comment {
    position: fixed; z-index: 2147482999; text-align: center; color: #fff; font-size: 13px;
    text-shadow: 0 0 3px #000, 0 0 3px #000; pointer-events: none;
  }
  .comment.mistake { color: #ff9a9a; }
  [hidden] { display: none !important; }
`;

const host = document.createElement("div");
host.id = "hanab-analysis";
const shadow = host.attachShadow({ mode: "open" });
const styleElement = document.createElement("style");
styleElement.textContent = STYLE;
shadow.append(styleElement);

const robot = document.createElement("button");
robot.className = "robot";
robot.textContent = "🤖";
robot.title =
  "Bot analysis: the bot goes through this game in your browser and marks every move it would have played differently. "
  + "Unofficial, and often wrong.";
robot.hidden = true;

const previousButton = document.createElement("button");
previousButton.className = "nav";
previousButton.textContent = "◀!";
previousButton.title = "The previous move the bot comments on";
previousButton.hidden = true;

const nextButton = document.createElement("button");
nextButton.className = "nav";
nextButton.textContent = "!▶";
nextButton.title = "The next move the bot comments on (the last stop is the summary)";
nextButton.hidden = true;

const ticks = document.createElement("div");
const comment = document.createElement("div");
comment.className = "comment";
comment.hidden = true;

shadow.append(robot, previousButton, nextButton, ticks, comment);

let panel: HTMLDivElement | null = null;
let progress: HTMLDivElement | null = null;

function mount() {
  (document.body ?? document.documentElement).append(host);
}
if (document.body === null) {
  document.addEventListener("DOMContentLoaded", mount);
} else {
  mount();
}

// -----------------------
// The page bridge's state
// -----------------------

window.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (event.source !== window) {
    return;
  }
  const message = event.data as Partial<PageMessage> | null;
  if (message?.source !== PAGE_SOURCE || message.type !== "state") {
    return;
  }

  page = { info: message.info ?? null, geometry: message.geometry ?? null };
  update();
});

function goTo(segment: number) {
  const message: GoToMessage = { source: CONTENT_SOURCE, type: "goto", segment };
  window.postMessage(message, window.location.origin);
}

/** The finished games that are in the database are the ones that "/export/<id>" has. */
function getAnalyzableInfo(): (GameInfo & { databaseID: number }) | null {
  const info = page.info;
  if (info === null || !info.finished || info.databaseID === null || info.databaseID <= 0) {
    return null;
  }
  return info as GameInfo & { databaseID: number };
}

/** The segment to show a comment at: the position just before the move, as the player saw it. */
function getSegment(c: AnalysisComment): number {
  return c.turn - 1;
}

/** The stops of the `◀!` and `!▶` buttons: the turns with a comment, and the end (the summary). */
function getStops(analysis: AnalysisResult, finalSegment: number): number[] {
  return [...new Set([...analysis.comments.map(getSegment), finalSegment])].sort((a, b) => a - b);
}

// -------------------
// Updating the screen
// -------------------

function place(element: HTMLElement, left: number, top: number) {
  element.style.left = `${Math.round(left)}px`;
  element.style.top = `${Math.round(top)}px`;
}

function update() {
  const info = getAnalyzableInfo();
  if (info === null || info.databaseID !== resultGameID) {
    if (result !== null && (info === null || info.databaseID !== resultGameID)) {
      clearResult();
    }
  }

  const supported = info === null ? false : isSupported(info);
  const geometry = page.geometry;
  const visible = info !== null && supported && geometry !== null;
  robot.hidden = !visible;
  if (!visible) {
    previousButton.hidden = true;
    nextButton.hidden = true;
    ticks.replaceChildren();
    comment.hidden = true;
    closePanel();
    return;
  }

  // The robot is above the right end of the replay bar, with the jump buttons to its left.
  const top = geometry.bar.y - 34;
  const right = geometry.bar.x + geometry.bar.width;
  place(robot, right - 28, top);
  place(nextButton, right - 28 - 32, top);
  place(previousButton, right - 28 - 64, top);

  previousButton.hidden = true;
  nextButton.hidden = true;
  if (result !== null && result.comments.length > 0) {
    const stops = getStops(result, info.finalSegment);
    previousButton.hidden = false;
    nextButton.hidden = false;
    previousButton.disabled = !stops.some((s) => s < info.segment);
    nextButton.disabled = !stops.some((s) => s > info.segment);
  }

  drawTicks(info, geometry);
  drawComment(info, geometry);
}

function drawTicks(info: GameInfo, geometry: NonNullable<PageState["geometry"]>) {
  ticks.replaceChildren();
  if (result === null) {
    return;
  }

  const { bar, shuttle } = geometry;
  const finalSegment = Math.max(info.finalSegment, 1);
  const sliderWidth = bar.width - shuttle.width;
  const width = Math.max(4, Math.round(bar.height * 0.9));
  const height = Math.max(7, Math.round(bar.height * 1.4));
  // Below the shuttle, so that the shuttle is never covered.
  const top = bar.y + bar.height / 2 + shuttle.height / 2 + 1;

  const add = (segment: number, color: string, title: string) => {
    const tick = document.createElement("div");
    tick.className = "tick";
    tick.style.background = color;
    tick.style.width = `${width}px`;
    tick.style.height = `${height}px`;
    tick.title = title;
    place(tick, bar.x + (sliderWidth / finalSegment) * segment + shuttle.width / 2 - width / 2, top);
    tick.addEventListener("click", () => {
      goTo(segment);
    });
    ticks.append(tick);
  };

  // Mistakes last, so that they are on top if a suggestion is on the same turn.
  const ordered = result.comments.toSorted((a, b) => Number(a.mistake) - Number(b.mistake));
  for (const c of ordered) {
    add(getSegment(c), c.mistake ? MISTAKE_COLOR : SUGGESTION_COLOR, `Turn ${c.turn}: ${c.text}`);
  }
  add(info.finalSegment, SUMMARY_COLOR, "The bot's summary of the game");
}

function getSummary(analysis: AnalysisResult): string {
  const total = analysis.comments.length;
  const mistakes = analysis.comments.filter((c) => c.mistake).length;
  const label = `Bot analysis (${resultConvention})`;
  if (total === 0) {
    return `${label}: the bot would not have played any move differently.`;
  }

  const moves = total === 1 ? "1 move differs" : `${total} moves differ`;
  const clear = mistakes === 1 ? "1 clear mistake" : `${mistakes} clear mistakes`;
  return `${label}: ${moves} from what the bot would do, ${clear}.`;
}

function drawComment(info: GameInfo, geometry: NonNullable<PageState["geometry"]>) {
  if (result === null) {
    comment.hidden = true;
    return;
  }

  const here = result.comments.filter((c) => getSegment(c) === info.segment);
  let text: string;
  if (here.length > 0) {
    text = here.map((c) => `${result?.players[c.player] ?? "?"}: ${c.text}`).join("\n");
  } else if (info.segment === info.finalSegment) {
    text = `${getSummary(result)}\n(Unofficial; the bot is often wrong.)`;
  } else {
    comment.hidden = true;
    return;
  }

  comment.hidden = false;
  comment.textContent = text;
  comment.style.whiteSpace = "pre-line";
  comment.classList.toggle("mistake", here.some((c) => c.mistake));
  comment.style.width = `${geometry.replayArea.width}px`;
  place(comment, geometry.replayArea.x, geometry.replayArea.y + geometry.replayArea.height * 0.9);
}

function clearResult() {
  result = null;
  resultGameID = null;
  ticks.replaceChildren();
  comment.hidden = true;
}

// -----------------
// The bot's worker
// -----------------

let worker: Worker | null = null;
let onSupports: ((supported: boolean) => void) | null = null;
let onAnalysisMessage: ((message: WorkerResponse) => void) | null = null;

function extensionURL(file: string): string {
  return chrome.runtime.getURL(file);
}

/**
 * Starts the worker. A page can't load a worker from an extension's URL, so it is started from a
 * Blob that holds the worker's code, and the WebAssembly file is sent to it.
 */
async function startWorker(): Promise<Worker> {
  if (worker !== null) {
    return worker;
  }

  const [code, wasm] = await Promise.all([
    fetch(extensionURL("worker.js")).then(async (r) => r.text()),
    fetch(extensionURL("hanab_bot_wasm_bg.wasm")).then(async (r) => r.arrayBuffer()),
  ]);
  if (worker !== null) {
    return worker;
  }

  const url = URL.createObjectURL(new Blob([code], { type: "text/javascript" }));
  const newWorker = new Worker(url, { type: "module" });
  newWorker.addEventListener("message", (event: MessageEvent<WorkerResponse>) => {
    const message = event.data;
    if (message.type === "supports") {
      onSupports?.(message.supported);
    } else {
      onAnalysisMessage?.(message);
    }
  });
  newWorker.addEventListener("error", () => {
    // The bot could not be loaded, or it stopped.
    onSupports?.(false);
    onAnalysisMessage?.({ type: "error", message: "The analysis stopped unexpectedly." });
  });
  const init: WorkerRequest = { type: "init", wasm };
  newWorker.postMessage(init, [wasm]);
  worker = newWorker;
  return newWorker;
}

function send(target: Worker, request: WorkerRequest) {
  target.postMessage(request);
}

/** Ends the worker (it is the only way to stop an analysis); the next question starts a new one. */
function stopWorker() {
  worker?.terminate();
  worker = null;
  onSupports = null;
  onAnalysisMessage = null;
}

/**
 * Whether the bot can follow the game, which only the bot knows (it can't play some variants and
 * table options). The first time this is asked about a kind of game it starts the bot and answers
 * false until the bot has replied, which then updates the screen.
 */
function isSupported(info: GameInfo): boolean {
  const query = {
    variant: info.variant,
    emptyClues: info.emptyClues,
    detrimentalCharacters: info.detrimentalCharacters,
    oneExtraCard: info.oneExtraCard,
    oneLessCard: info.oneLessCard,
  };
  const key = JSON.stringify(query);
  const known = supportCache.get(key);
  if (known !== undefined) {
    return known;
  }

  if (onSupports === null) {
    onSupports = (supported) => {
      onSupports = null;
      supportCache.set(key, supported);
      update();
    };
    startWorker()
      .then((started) => {
        send(started, { type: "supports", ...query });
      })
      .catch(() => {
        onSupports?.(false);
      });
  }
  return false;
}

// -------------------------------
// The box next to the robot button
// -------------------------------

function closePanel() {
  panel?.remove();
  panel = null;
}

robot.addEventListener("click", () => {
  if (panel !== null) {
    closePanel();
    return;
  }

  const picker = document.createElement("select");
  for (const [value, label] of CONVENTIONS) {
    picker.append(new Option(label, value));
  }
  picker.value = localStorage.getItem(CONVENTION_KEY) ?? "HGroup3";
  const start = document.createElement("button");
  start.textContent = "Analyze";
  start.style.marginLeft = "6px";
  start.addEventListener("click", () => {
    const label = picker.selectedOptions[0]?.textContent ?? picker.value;
    localStorage.setItem(CONVENTION_KEY, picker.value);
    closePanel();
    run(picker.value, label).catch((error: unknown) => {
      showError(String(error));
    });
  });

  const title = document.createElement("div");
  title.textContent = "Bot analysis";
  title.style.fontWeight = "bold";
  const small = document.createElement("div");
  small.className = "small";
  small.textContent = DISCLAIMER;

  panel = document.createElement("div");
  panel.className = "panel";
  panel.append(title, picker, start, small);
  shadow.append(panel);

  const box = robot.getBoundingClientRect();
  place(panel, Math.max(box.right - 300, 4), box.bottom + 6);
});

previousButton.addEventListener("click", () => {
  jump(-1);
});
nextButton.addEventListener("click", () => {
  jump(1);
});

function jump(direction: -1 | 1) {
  const info = page.info;
  if (info === null || result === null) {
    return;
  }

  const stops = getStops(result, info.finalSegment).filter((s) =>
    direction > 0 ? s > info.segment : s < info.segment,
  );
  const target = direction > 0 ? stops[0] : stops.at(-1);
  if (target !== undefined) {
    goTo(target);
  }
}

// -------------------------------------
// The progress bar in the middle of the screen
// -------------------------------------

function dismissProgress() {
  progress?.remove();
  progress = null;
}

function showProgress(): { text: HTMLElement; bar: HTMLProgressElement } {
  dismissProgress();

  const text = document.createElement("div");
  const bar = document.createElement("progress");
  const cancel = document.createElement("button");
  cancel.textContent = "Cancel";
  cancel.addEventListener("click", () => {
    stopWorker();
    dismissProgress();
  });

  progress = document.createElement("div");
  progress.className = "progress";
  progress.append(text, bar, cancel);
  shadow.append(progress);
  return { text, bar };
}

/** An error stays on the screen for a few seconds, or until it is clicked. */
function showError(message: string) {
  stopWorker();
  const { text, bar } = showProgress();
  bar.remove();
  progress?.querySelector("button")?.remove();
  text.className = "error";
  text.textContent = `The analysis failed: ${message}`;
  const element = progress;
  element?.addEventListener("click", dismissProgress);
  setTimeout(() => {
    if (progress === element) {
      dismissProgress();
    }
  }, 8000);
}

// -----------------
// Running the bot
// -----------------

async function run(convention: string, conventionLabel: string) {
  const info = getAnalyzableInfo();
  if (info === null) {
    showError("This game can not be analyzed.");
    return;
  }

  const { text, bar } = showProgress();
  try {
    text.textContent = "Loading the game...";
    const response = await fetch(`/export/${info.databaseID}`);
    if (!response.ok) {
      throw new Error(`The server could not export game ${info.databaseID}.`);
    }
    const exportJson = await response.text();

    const started = await startWorker();
    const analysis = await new Promise<AnalysisResult>((resolve, reject) => {
      onAnalysisMessage = (message) => {
        if (message.type === "progress") {
          text.textContent = `Bot analysis: move ${message.done} of ${message.total}`;
          bar.max = message.total;
          bar.value = message.done;
        } else if (message.type === "result") {
          resolve(message.result);
        } else if (message.type === "error") {
          reject(new Error(message.message));
        }
      };
      text.textContent = "Bot analysis: starting...";
      send(started, {
        type: "analyze",
        exportJson,
        variant: info.variant,
        convention,
        // The browser is slower than a native bot; a search that runs out of time reads as a mistake.
        endgameTimeoutMs: 2000,
      });
    });
    onAnalysisMessage = null;

    result = analysis;
    resultGameID = info.databaseID;
    resultConvention = conventionLabel;
    dismissProgress();
    update();
  } catch (error) {
    // A cancelled analysis has already dismissed its progress bar and ended its worker.
    if (progress !== null) {
      showError(error instanceof Error ? error.message : String(error));
    }
  }
}
