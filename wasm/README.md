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

## Releasing it

A web project should depend on the module like on any other package, not keep the generated files in its source. Push a tag
like `wasm-v0.1.0` (the version in `Cargo.toml`) and `.github/workflows/wasm-release.yml` builds it and publishes a release
with `hanab-bot-wasm-0.1.0.tgz`, an npm package, and its `.sha256`. In the project's `package.json`:

```json
"hanab-bot-wasm": "https://github.com/michielbaird/hanab-bot-rs/releases/download/wasm-v0.1.0/hanab-bot-wasm-0.1.0.tgz"
```

and `npm install` records the checksum in the lock file. `./package.sh` makes the same package locally in `dist/` (for
trying a change: `npm install <path to the .tgz>`); the archive is the same for the same build, because it has no
timestamps or owners in it.
