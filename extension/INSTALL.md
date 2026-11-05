# Installing the extension

The extension is not in any store. You build it and load it from a folder. The bot can be wrong (see [README.md](README.md)).

## 1. Build it

You need Node 22 or newer and network access (the build downloads the site's variant data once).

```sh
git clone git@github.com:michielbaird/hanab-bot-rs.git
cd hanab-bot-rs
git checkout extension        # until it is merged
cd extension
npm ci
npm run build
```

This makes `dist/chrome` (Chrome, Edge) and `dist/firefox`, both for https://hanab.live.

To use it on another copy of the site, such as a development instance, list every address (comma-separated match patterns):

```sh
EXT_MATCHES="https://hanab.live/*,http://localhost/*" npm run build
```

## 2. Load it

**Chrome or Edge**

1. Open `chrome://extensions` (Edge: `edge://extensions`).
2. Turn on **Developer mode** (top right).
3. Click **Load unpacked** and pick the `dist/chrome` folder (the one with `manifest.json`). Keep the folder: the browser
   reads the extension from it.
4. "Hanab Analysis" should be listed without errors. After a rebuild, click the reload arrow on its card.

**Firefox (128 or newer)**

1. Open `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on** and pick `dist/firefox/manifest.json`.
3. A temporary add-on is removed when Firefox closes; load it again next time. (Firefox is untested.)

## 3. Use it

1. Open the replay of a **finished game** on hanab.live, for example from your history. Reload the page once after
   installing the extension: content scripts only attach to pages loaded after it.
2. The robot button appears above the right end of the replay bar after a moment (it only shows for finished games that are in the database; for a variant or table option that the bot can't play, the box says so).
3. Click it. The box checks that the bot can play the game (the first time, this loads the bot). Pick a convention (the default is H-Group level 3), and click **Analyze**. A progress bar is shown in the middle
   of the screen; a long game takes about half a minute.
4. Afterwards:
   - markers above the replay bar: dark orange and tall for a clear mistake, yellow and short for a suggestion, blue for the summary at the end of the
     game. Click one to go there;
   - `◀!` and `!▶` next to the robot jump to the previous and next comment, the last stop being the end of the game;
   - the bot's comment for the turn you are on is shown under the replay controls, and the summary at the end.
5. The toolbar button has the settings (off, starting convention, slow device, only clear mistakes). Analyses are kept in your
   browser, so opening the game again shows the result again.

## If it doesn't work

- **No robot.** Check that the page is the replay of a finished game and reload it. Then open the browser's developer tools
  (F12), look for red errors in the Console, and try `window.globals?.store?.getState().replay`: if that is `undefined`, the
  site's internals are not what the extension expects (it reads them from `window.globals`).
- **The robot, markers or comment overlap something.** Their positions come from where the site draws the replay controls on
  its canvas. Tell the maintainer, with a screenshot.
- **"The analysis failed".** The message says why: the game could not be exported, or the bot could not follow the game
  (some games, especially in odd variants, make it fail). Try another convention.
- **Remove it.** `chrome://extensions` and **Remove**, or close Firefox.
