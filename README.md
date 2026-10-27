# hanab-bot-rs

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

## Docker

```
docker compose up -d --build        # builds the image and starts the bots (accounts from .env, see docker-compose.yml)
docker compose logs -f bots
docker compose run --rm analyze users=bot1,bot2,bot3 since=2026-10-27   # analyze the bots' games
```

Or without compose: `docker run -d -e PUID=1026 -e PGID=100 --env-file .env -v /volume1/docker/hanab:/data hanab-bot-rs bot index=0,1,2`.

- **Command:** the image has no default command; give the program and its arguments (`bot index=0,1,2`, or `bot index=0` for one account).
- **User:** the container starts as root only to make `/data` belong to `PUID:PGID` (default `10001:10001`), then drops to that user. Set both to
  the owner of the data folder on the host (on a NAS, the UID/GID of the account that owns the share). If the platform already starts the
  container as a non-root user (`--user`, compose `user:`), the entrypoint just runs the command as that user.
- **Data:** `/data` is the working directory; `feedback.jsonl` and the analysis output (`/data/analysis`) live there.

## Feedback

When a game starts the bot tells the table: *"I'm an unofficial Rust port of scala-bot. If I'm behaving weirdly, please send feedback with:
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

Endgame "mistake!" comments about *people's* moves can be spurious (the bot's model of a person's hand follows its own conventions); they are reliable for the bots' own moves.
`ANALYZE_ENDGAME_TIMEOUT_MS` (default 500) sets how long each endgame winrate may take.
