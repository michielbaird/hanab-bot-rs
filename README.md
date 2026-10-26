# scala-bot-rs

An unofficial Rust port of [scala-bot](https://github.com/will-hanabi-bot/scala-bot), a deterministic Hanabi bot for
[hanab.live](https://hanab.live). It plays H-Group conventions (levels 1-12), Referential Sieve and Reactor. GPLv3, like the original.

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

## Feedback

When a game starts the bot tells the table: *"I'm an unofficial Rust port of scala-bot. If I'm behaving weirdly, please send feedback with:
/pm <bot> /feedback <what happened>"*. `/feedback <message>` (by PM) appends a JSON line to `feedback.jsonl` with the sender, the message, the bot's
convention and version, the table, variant, players, turn and the last 10 actions. Messages are limited to 1000 characters and 5 per person per hour.

| Environment variable | Effect |
|---|---|
| `HANABI_FEEDBACK_FILE` | file to append to (default `feedback.jsonl`; `off` or empty turns `/feedback` off) |
| `HANABI_FEEDBACK_INTRO` | `0` stops the announcement at the start of each game (default `1`) |
