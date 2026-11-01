# hanab-bot-wasm

The bot's analysis of a finished game ("every move the bot would have played differently"), compiled to WebAssembly
so a web page can run it, with no server.

```js
import init, { init_variants, analyze } from "./hanab_bot_wasm.js";
await init();                                   // inside a Web Worker: analysis blocks for many seconds
init_variants(variantsJson, suitsJson);         // hanabi-live's packages/game/src/json/{variants,suits}.json
const result = JSON.parse(analyze(exportJson, "No Variant", "HGroup3", 2000, (done, total) => {}));
// { players: [...], comments: [{ turn, player, mistake, text }] }
```

* `exportJson` is the game as hanab.live exports it (`/export/<id>`): `players`, `deck`, `actions`.
* The endgame timeout (2000 above; 0 is the default 500 ms) is per winrate; slow devices want a longer one.
* There are no threads in the browser, so the players are replayed one after another; expect about 25 s for a
  72-move game on a desktop (about 4x what the native `analyze` takes on 4 cores).
* `panic = "abort"`: a bug in the bot kills the module. Run it in a Worker and start a new one.

`./build.sh` builds `pkg/` (about 1.6 MB of wasm). The output was checked against the native `analyze` binary on
`investigations/replay_1970135.json` (identical comments).
