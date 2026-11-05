# Store listing

Text for the Chrome Web Store and Firefox Add-ons listings, and the answers the stores ask for.

## Name

Hanab Analysis

## Summary (132 characters at most)

A bot marks the moves it would have played differently in a finished hanab.live game. It can be wrong.

## Description

Hanab Analysis has a bot go through a finished game on hanab.live and mark the moves it would have played differently.

The bot can be wrong, especially at higher H-Group levels.

- Open the replay of a finished game and click the robot above the replay bar.
- Pick a convention (H-Group level 1 to 12, Reactor or Referential Sieve) and start the analysis.
- Every move the bot would have played differently gets a marker above the replay bar, and its comment is shown under
  the replay controls. Buttons jump between them, and the end of the game has a summary.
- It runs in your browser. Nothing about the game is sent anywhere, and nothing is collected.
- Settings in the toolbar popup: switch it off, change the starting convention, give endgame searches more time on a slow
  device, or show only the clear mistakes.

It only works on hanab.live, on finished games, and for the variants and options that the bot can play.

## Single purpose

Show a bot's analysis of a finished hanab.live game on its replay.

## Permissions

- **Access to hanab.live** (content scripts on `https://hanab.live/*`): to draw the robot, markers and comments on the
  replay page, and to ask the site for the game's export (`/export/<game id>`), which the analysis needs.
- **A script in the page's own context** (`"world": "MAIN"`): the replay state and the position of the replay controls are
  kept in the site's own JavaScript objects, which a normal content script can't read. This script only reads them and moves
  the replay when you use a jump button; it sends nothing anywhere.
- **`storage`**: to keep the settings and the analyses you have run, on your device.
- **Web accessible resources** (the worker and the `.wasm` file): the bot runs in a worker that the page loads from a Blob, so
  it needs these two files.

## Data usage (Chrome)

No user data is collected, sold, or transferred. Not used for anything unrelated to the single purpose. Privacy policy:
`PRIVACY.md` in this directory. (Firefox: `data_collection_permissions` is `none`.)

## Screenshots

To be taken on the real site (1280x800 for Chrome): the robot and box, a replay with the markers and a comment, the end-of-game
summary. The icon is `icons/icon-128.png`.

## For the review

`SOURCE.md` says how to build the submitted files from source; `THIRD_PARTY.md` lists what is bundled.
