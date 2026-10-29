# hanab-bot-rs

[![CI](https://github.com/michielbaird/hanab-bot-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/michielbaird/hanab-bot-rs/actions/workflows/ci.yml)

An unofficial Rust port of [scala-bot](https://github.com/will-hanabi-bot/scala-bot), a deterministic Hanabi bot for
[hanab.live](https://hanab.live). It plays H-Group conventions (levels 1-12), Referential Sieve and Reactor. GPLv3, like the original. Repository: <https://github.com/michielbaird/hanab-bot-rs>.

```
cargo build --release
cargo test -j2

# self-play (no network)
cargo run --release --bin self_play -- games=10 players=3 convention=HGroup5

# the hanab.live bot: put HANABI_USERNAME0 / HANABI_PASSWORD0 (and optionally HANABI_HOSTNAME) in ./.env
cargo run --release --bin bot -- index=0

# several bots in one process (HANABI_USERNAME0/1/2 ...): one login, connection and decision thread each
cargo run --release --bin bot -- index=0,1,2
```

Hosting several bots in one process shares the (small) variant tables and the runtime; a bot's memory is a few MB.
With several bots, console commands take the bot's index or username first (`1 hand Bob`). Logging in the live bot is compiled out unless
you build with `--features verbose-logs`.

Then PM the bot `/settings HGroup5` (or `Reactor1`, `RefSieve`, ...), and create a table and invite or `/join` it.
Other tools: `replay`, `analyze` (exported games).

See `HANDOFF.md` for the current state and how to continue, `PLAN.md` for the milestones, `PORTING_NOTES.md` for the design
and Scala-vs-Rust differences, and `FIX_LATER.md` for Scala oddities noted during the port.

## Using it as a library

The `net` feature (on by default) is everything that touches the network: the `bot` and `analyze_history` binaries, which use tokio, a websocket
client and an HTTP client, and downloading a game by replay id. Without it the crate depends on serde and nothing else, so it can be embedded in
another program (an app, a different transport) that owns the connection:

```toml
hanab-bot-rs = { git = "https://github.com/michielbaird/hanab-bot-rs", default-features = false }
```

```rust
use std::sync::mpsc;
use hanab_bot::basics::Variant;
use hanab_bot::client::{BotClient, BotConfig, Out};

// Once per process: the contents of hanab.live's variants.json and suits.json (fetched by you).
Variant::init(&variants_json, &suits_json)?;

let (tx, rx) = mpsc::channel();
let mut client = BotClient::new(BotConfig::default(), tx);

// For every text message the websocket delivers (this can take seconds when the bot is choosing a move,
// so call it from a worker thread, not a UI thread):
client.handle_msg(&text);

// Whatever the bot wants to say back, in order:
for out in rx.try_iter() {
    match out {
        Out::Send(text) => { /* write it to the websocket */ }
        Out::Wait(pause) => { /* wait this long before the next one (the bot paces its moves) */ }
    }
}
```

Analysing a finished game (`Game::analyze_comments_with`, after rebuilding it with `tools::game_data::build_game`) replays it once per player,
on one thread per player by default (`threads: 1` keeps it on the calling thread), and can report progress and be cancelled:

```rust
use std::sync::atomic::AtomicBool;
use hanab_bot::basics::game::{AnalysisOptions, AnalysisProgress, Cancelled};

let cancel = AtomicBool::new(false); // set it from another thread (e.g. the UI) to stop
let progress = |p: AnalysisProgress| { /* p.moves_done out of p.moves_total */ };
match game.analyze_comments_with(&AnalysisOptions { progress: Some(&progress), cancel: Some(&cancel), ..Default::default() }) {
    Ok(comments) => { /* each has a turn, player_index, mistake flag and text */ }
    Err(Cancelled) => {}
}
```

Logging into hanab.live (the `POST /login` that returns the session cookie) and the websocket itself are the embedder's job; `src/bin/bot.rs`
shows the whole thing, including reconnecting and pacing. Several `BotClient`s can run in one process, each on its own thread. The `/analyze`
chat command needs the `net` feature and answers that it is unavailable without it.

## Docker

```
docker build -t hanab-bot-rs .

# the bots: accounts come from the environment (HANABI_USERNAME0/PASSWORD0, ...), see below for PUID/PGID
docker run -d --name hanab-bots --restart unless-stopped \
    -e PUID=1026 -e PGID=100 --env-file .env -v /volume1/docker/hanab:/data \
    hanab-bot-rs bot index=0,1,2

# analyze the bots' games (reads /data/feedback.jsonl, writes /data/analysis)
docker run --rm -e PUID=1026 -e PGID=100 -v /volume1/docker/hanab:/data \
    hanab-bot-rs analyze_history users=bot1,bot2,bot3 since=2026-10-27
```

- **Command:** the image has no default command; give the program and its arguments (`bot index=0,1,2`, or `bot index=0` for one account).
  Programs: `bot`, `analyze_history`, `analyze`, `replay`, `self_play`.
- **User:** the container starts as root only to make `/data` belong to `PUID:PGID` (default `10001:10001`), then drops to that user. Set both to
  the owner of the data folder on the host (on a NAS, the UID/GID of the account that owns the share). If the platform already starts the
  container as a non-root user (`--user`), the entrypoint just runs the command as that user and `/data` must be writable by it.
- **Data:** `/data` is the working directory; `feedback.jsonl` and the analysis output (`/data/analysis`) live there.

## Development

CI (`.github/workflows/ci.yml`) runs on every push to `main` and every pull request:

| Job | What it checks | Same thing locally |
|---|---|---|
| test | all tests, with and without the `net` feature | `cargo test --locked` and `cargo test --locked --no-default-features` |
| fmt and clippy | formatting and no clippy warnings, both feature sets | `cargo fmt --check`; `cargo clippy --locked --all-targets -- -D warnings` (and with `--no-default-features`) |
| library has no network dependencies | the library without `net` builds and its dependency tree has no tokio, reqwest, hyper, tungstenite or futures | `cargo tree --no-default-features -e normal` |
| docker image | the image builds; no command gives the usage message; `PUID`/`PGID` are honoured and own `/data`; the bot starts | `docker build -t hanab-bot-rs .` |

## Benchmark

`benchmark` plays the same seeded self-play games repeatedly and prints the time per repetition, a fingerprint of every move played (to check that
a change did not alter the bot's behaviour) and, with `--features stats`, counts of the work done. See [PERFORMANCE.md](PERFORMANCE.md).

## Reporting problems

Open an issue at <https://github.com/michielbaird/hanab-bot-rs/issues> (the bug report template asks for the replay link, which bot and
convention, and what you expected). In a game you can also PM the bot `/feedback <what happened>`. Known problems, including the ones the
original scala-bot still has open, are tracked there too.

## Feedback

When a game starts the bot tells the table (once per person per 24 hours, so "Play again", which keeps the chat, doesn't repeat it): *"I'm an unofficial Rust port of scala-bot. If I'm behaving weirdly, please send feedback with:
/pm <bot> /feedback <what happened>"*. `/feedback <message>` (by PM) appends a JSON line to `feedback.jsonl` with the sender, the message, the bot's
convention and version, the table, variant, players, turn and the game's seed. The game itself is in the bot's history on hanab.live. Messages are limited to 1000 characters and 5 per person per hour.

| Environment variable | Effect |
|---|---|
| `HANABI_FEEDBACK_FILE` | file to append to (default `feedback.jsonl`; `off` or empty turns `/feedback` off) |
| `HANABI_ENDGAME_TIMEOUT_MS` | how long the endgame solver may search per move (default `3000`; self-play often hits the limit, so raise it if you have time to spare) |
| `HANABI_FEEDBACK_INTRO` | `0` stops the announcement at the start of each game (default `1`) |

## Analyzing the bots' games

```
cargo run --release --bin analyze_history -- users=bot1,bot2,bot3 since=2026-10-27 feedback=feedback.jsonl out=analysis threads=2
```

Lists each user's games from the public hanab.live history, fetches every replay, and analyzes it with the convention the bot wrote in its first-card
note (`convention=` is the fallback). Per game it writes `analysis/<id>.txt` (readable) and `<id>.json`; games already analyzed are reused
(`force=1` redoes them, e.g. after upgrading the bot). `analysis/summary.md` has a table per convention (average score, perfect games, strikeouts,
how often the bot disagrees with its own moves and with everyone else's) and a list of games worth a look, with any `/feedback` that carries the
same seed. A disagreement with the bot's *own* move means the bot has changed since or isn't deterministic; disagreements with humans are expected.

Endgame "mistake!" comments compare the winrates of the bot's move and the player's move from the real position at that turn (everyone's hand revealed, including the mover's own), so they are about what really happens, not about what the bot's conventions would have predicted.
`ANALYZE_ENDGAME_TIMEOUT_MS` (default 500) sets how long each endgame winrate may take.
