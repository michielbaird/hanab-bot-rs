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
```

Then PM the bot `/settings HGroup5` (or `Reactor1`, `RefSieve`, ...), and create a table and invite or `/join` it.
Other tools: `replay`, `analyze` (exported games).

See `HANDOFF.md` for the current state and how to continue, `PLAN.md` for the milestones, `PORTING_NOTES.md` for the design
and Scala-vs-Rust differences, and `FIX_LATER.md` for Scala oddities noted during the port.
