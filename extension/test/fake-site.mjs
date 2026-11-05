// A stand-in for a hanab.live replay page, for the tests: it serves a page that sets "window.globals" with the same
// shape as the site's (only the parts that the extension reads), and the exported game at "/export/<id>".

import { readFileSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
export const GAME_ID = 1_970_135;
const EXPORT = readFileSync(
  path.join(here, "../../investigations/replay_1970135.json"),
  "utf8",
);

const PAGE = (variant) => `<!doctype html>
<html><head><meta charset="utf-8"><title>fake replay</title>
<style>html,body{margin:0;height:100%;background:#00420f}#game{position:absolute;inset:0}</style></head>
<body><div id="game"></div>
<script>
  const node = (x, y, width, height) => ({
    getAbsolutePosition: () => ({ x, y }), width: () => width, isVisible: () => true, height: () => height,
  });
  const state = {
    finished: true,
    replay: { active: true, segment: 0, databaseID: ${GAME_ID}, hypothetical: null, shared: null },
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
      replayBar: node(100, 300, 500, 8),
      replayShuttle: node(0, 0, 24, 24),
      replayArea: node(60, 260, 600, 220),
      replayBackFullButton: node(110, 340, 60, 40),
      enterHypoButton: node(250, 420, 220, 40),
      pauseSharedTurnsButton: null,
      useSharedTurnsButton: null,
      replayExitButton: null,
    },
  };
</script></body></html>`;

export function startFakeSite(port) {
  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? "/", `http://localhost:${port}`);
    if (url.pathname.startsWith("/replay/")) {
      response.setHeader("content-type", "text/html");
      response.end(PAGE(url.searchParams.get("variant") ?? "6 Suits"));
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
