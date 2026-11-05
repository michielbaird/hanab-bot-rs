# Hanab Analysis (unofficial browser extension)

Has the bot of this repository look through a **finished game on hanab.live** and mark the moves it would have played
differently. It runs entirely in your browser (the bot is WebAssembly, see `../wasm`); the only request it makes to the
site is the usual `/export/<id>` of the game you are looking at.

> **Unofficial.** Not made or endorsed by hanab.live or the H-Group. The bot is often wrong, especially at higher H-Group
> levels, and so are its suggestions: treat them as a second opinion. The extension says so wherever it shows them.

## What it does

On the replay of a finished game (one in the database):

- a robot button above the right end of the replay bar opens a small box to pick the convention (H-Group level 1 to 12,
  Reactor or Referential Sieve) and start the analysis, with a progress bar in the middle of the screen. The bot is only
  loaded when you open the box, which also says if it can't play the game's variant or table options. The first time, you
  tick that you understand that this is unofficial and the bot often wrong;
- every move the bot would have played differently gets a marker above the replay bar (dark orange and tall for a clear
  mistake, yellow and short for a suggestion, blue for the end of the game; the colours are chosen to stay apart with
  colour blindness), and the bot's comment is shown under the replay controls while you are on that turn;
- `◀!` and `!▶` jump to the previous and next comment; the last stop is the end of the game, where a summary of the whole
  analysis is shown. Clicking (or tabbing to and pressing Enter on) a marker also jumps there;
- the analyses are kept in your browser, so opening the game again shows the result again;
- the toolbar button has the settings: switch the extension off, the convention the box starts on, a slow-device mode that
  gives each endgame search more time, and "only show clear mistakes".

The box also has a link to a new GitHub issue with the game, variant and convention filled in.

## Building and installing

See [INSTALL.md](INSTALL.md). For the stores: [STORE.md](STORE.md) (listing and permission justifications), [PRIVACY.md](PRIVACY.md),
[SOURCE.md](SOURCE.md) and [THIRD_PARTY.md](THIRD_PARTY.md). A tag `extension-v<version>` publishes the packages (see
`.github/workflows/extension-release.yml`). In short: `npm ci && npm run build` makes `dist/chrome` and `dist/firefox`, which you load as
an unpacked extension (Chrome, Edge) or a temporary add-on (Firefox 128 or newer).

The build needs the network once: it downloads the site's `variants.json` and `suits.json` (a pinned commit of the
hanabi-live repository, see `build.mjs`) into `build/`. `hanab-bot-wasm` is the release package built by
`.github/workflows/wasm-release.yml`.

## How it works

The site keeps its game in objects that an extension's normal content script can't see, so there are two scripts:

- `src/page.ts` runs in the page's own context (`"world": "MAIN"`). It reads the replay state and the position of the replay
  bar from the site's `window.globals`, and moves the replay the way the site's own code does. **This depends on the
  site's internals** (`globals.store`, the Konva elements of the replay controls) and will break when they change.
- `src/content/` is the extension's part, kept apart by what each piece does:
  - `state.ts` and `store.ts`: one state object (page info, what the bot can play, the analysis, the box and progress
    that are open) that tells its listeners what changed;
  - `bridge.ts`: the messages to and from `page.ts`;
  - `engine.ts`: the bot as a client with `supports()`, `analyze()` and `cancel()`, all promises. A page can't load a worker
    from an extension's URL, so the worker (`src/worker.ts`) is started from a Blob and gets the WebAssembly file as its
    first message. If the site ever sets a content security policy without `blob:` and `wasm-unsafe-eval`, that stops working;
  - `controller.ts`: what the user can do (open the box, analyze, cancel, jump) and what follows from the state changing;
  - `analysis.ts` and `layout.ts`: pure functions (which turns to stop at, the summary text, where the markers and buttons
    go), unit-tested;
  - `ui/`: one small module per widget (robot and jump buttons, markers, comment, box, progress) that renders from the
    state, in a shadow root (so the site's styles don't touch it) styled by `src/content.css`.

## Tests

`npm run typecheck` checks the types and `npm run test:unit` runs the unit tests of the logic (no browser needed). `npm test` is an end-to-end test in a real Chromium: it loads the extension built
for a stand-in replay page (`test/fake-site.mjs`, which has the same `window.globals` shape as the site's) and goes through
an analysis of a real game.

```sh
EXT_MATCHES="http://localhost:8099/*" npm run build && npm test
```

It needs a Chromium that Playwright can find (`npx playwright-core install chromium`, and its system libraries, and some
fonts). It is not run in CI. It has **not** been tried against the real site, or in Firefox.
