# Privacy

Hanab Analysis (unofficial) collects no data about you, and sends none anywhere.

- **What it reads:** on hanab.live, the replay of a finished game that you open: which game it is, its variant and
  options, which turn is shown and where the replay controls are drawn on the page.
- **What it requests:** for a game you ask it to analyze, the exported game from hanab.live (`/export/<game id>`), the same
  request the site's own export link makes. Nothing else is requested, and nothing is sent to any other server.
- **Where the analysis runs:** in your browser (the bot is WebAssembly). Nothing about the game leaves it.
- **What it stores:** in your browser's extension storage on your device only: its settings, and the analyses you have run
  (the last 40 games), so that opening a game again shows the result again. Removing the extension removes them.
- **What it does not do:** no accounts, no analytics, no tracking, no advertising, no remote code. It does not read
  chat, other players' private information, or anything on other sites.

If you report a problem, the link opens a new issue on GitHub with the extension version, the game's address, its
variant and the convention filled in; nothing is sent unless you submit that issue.

The extension is unofficial: it is not made or endorsed by hanab.live or the H-Group, and the bot is often wrong.

Questions: [open an issue](https://github.com/michielbaird/hanab-bot-rs/issues).
