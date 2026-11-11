// A stand-in for a hanab.live replay page, for the tests: it serves a page that sets "window.globals" with the same
// shape as the site's (only the parts that the extension reads), and the exported game at "/export/<id>" (in the site's new shape).

import { readFileSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { getSiteLayout } from "./site-layout.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
export const GAME_ID = 1_970_135;
/** A saved game of before November 2026, in the shape that the site now exports: named turns, `playerNames`, `variantName`. */
function newExport(old) {
  const turn = (a) =>
    ({
      0: { type: "play", order: a.target },
      1: { type: "discard", order: a.target },
      2: { type: "clue", receiverIndex: a.target, clue: { type: "color", value: a.value } },
      3: { type: "clue", receiverIndex: a.target, clue: { type: "rank", value: a.value } },
    })[a.type];
  const { variant, ...flags } = old.options ?? {};
  return {
    gameId: GAME_ID,
    seed: old.seed,
    playerNames: old.players,
    deck: old.deck,
    actions: old.actions.map(turn),
    options: { variantName: variant ?? "No Variant", ...flags },
  };
}

const EXPORT = JSON.stringify(
  newExport(
    JSON.parse(
      readFileSync(
        path.join(here, "../../investigations/replay_1970135.json"),
        "utf8",
      ),
    ),
  ),
);

const PAGE = (variant, nodes, boxes) => `<!doctype html>
<html><head><meta charset="utf-8"><title>fake replay</title>
<style>html,body{margin:0;height:100%;background:#00420f}#game{position:absolute;left:0;top:0;width:${nodes.canvas.width}px;height:${nodes.canvas.height}px}.site{position:absolute;box-sizing:border-box;border:1px solid #fff8;background:#0006;color:#fff;font:11px sans-serif;overflow:hidden}</style></head>
<body><div id="game">${boxes}</div>
<script>
  const node = ({ x, y, width, height }, visible = true) => ({
    getAbsolutePosition: () => ({ x, y }), width: () => width, height: () => height, isVisible: () => visible, getStage: () => ({}),
  });
  const nodes = ${JSON.stringify(nodes)};
  const state = {
    finished: true,
    // (The site's state since November 2026: no "active", a "gameId", and "undefined" for what is not there.)
    replay: { segment: 0, gameId: ${GAME_ID}, hypothetical: undefined, shared: undefined },
    ongoingGame: { turn: { segment: 72 } },
    metadata: { options: { variantName: ${JSON.stringify(variant)}, emptyClues: false, detrimentalCharacters: false, oneExtraCard: false, oneLessCard: false } },
  };
  window.actions = [];
  window.globals = {
    store: {
      getState: () => state,
      dispatch: (action) => {
        window.actions.push(action);
        if (action.type === "replaySegment") state.replay.segment = action.segment;
      },
    },
    stage: { container: () => document.getElementById("game") },
    elements: {
      ...Object.fromEntries(Object.entries(nodes.elements).map(([name, rect]) => [name, node(rect)])),
      enterHypoButton: null, pauseSharedTurnsButton: null, useSharedTurnsButton: null, replayExitButton: null,
      ...Object.fromEntries(Object.entries(nodes.bottomRow).map(([name, rect]) => [name, node(rect)])),
    },
  };
</script></body></html>`;

export function startFakeSite(port) {
  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? "/", `http://localhost:${port}`);
    if (url.pathname.startsWith("/replay/")) {
      response.setHeader("content-type", "text/html");
      // ?w=&h=&kind= choose the size of the canvas and the kind of replay (see site-layout.mjs).
      const width = Number(url.searchParams.get("w") ?? 1280);
      const height = Number(url.searchParams.get("h") ?? 720);
      const layout = getSiteLayout(
        width,
        height,
        url.searchParams.get("kind") ?? "finished",
      );
      const boxes = Object.entries({
        ...layout.elements,
        ...layout.bottomRow,
        ...layout.others,
      })
        .filter(([name]) => name !== "replayShuttle" && name !== "replayArea")
        .map(
          ([name, r]) =>
            `<div class="site" style="left:${r.x}px;top:${r.y}px;width:${r.width}px;height:${r.height}px">${name}</div>`,
        )
        .join("");
      response.end(
        PAGE(url.searchParams.get("variant") ?? "6 Suits", layout, boxes),
      );
    } else if (url.pathname === `/export/${GAME_ID}`) {
      response.setHeader("content-type", "application/json");
      response.end(EXPORT);
    } else {
      response.statusCode = 404;
      response.end("not found");
    }
  });
  return new Promise((resolve) => {
    server.listen(port, () => {
      resolve(server);
    });
  });
}
