# Handoff: scala-bot → Rust rewrite

Written at the end of M1 so another session can pick this up cold. Read this, then `PLAN.md` (milestones)
and `PORTING_NOTES.md` (every deliberate difference from the Scala). Update this file at each checkpoint.

## The task

The user asked for a full Rust rewrite of https://github.com/will-hanabi-bot/scala-bot (a deterministic
Hanabi bot for hanab.live; conventions: H-Group up to level 12, Referential Sieve, Reactor).
The user's instructions, in their words or close to it:

- "Plan it out first. Set checkpoints and milestones and check in with me occasionally."
- "Use good Rust semantics" (idiomatic Rust, not a line-by-line transliteration), but behaviour must match the Scala.
- Start from scratch, borrowing selectively from rust-bot (the author's older, abandoned Rust port).
- Don't spawn subagents unless asked. Don't push or publish anything. Stop and wait for a go-ahead at each milestone.

## Where things are

| What | Path |
|---|---|
| The new crate (all work happens here) | `/workspace/scala-bot-rs` (git repo, 2 commits: M0 `f6acadd`, M1 `d44b182`) |
| Scala source (the spec) | `/workspace/scala-bot` (v1.2.8; `src/scala_bot` ~16.6k lines, `src/test` ~12.7k lines) |
| Older Rust port (reference only; Reactor only, ~9k lines) | `/workspace/rust-bot` |
| Rust toolchain | `~/.cargo/bin` (rustc/cargo 1.99.1). `export PATH=$HOME/.cargo/bin:$PATH` |
| JDK 25 + scala-cli | `~/tools` (installed for a `make` that was never run; unused, safe to delete) |

These live on the first session's machine. **A remote session will not see them unless the repos are
copied or pushed somewhere.** There is no git remote configured. `/workspace/scala-bot` and
`/workspace/rust-bot` are plain clones of the public repos and can simply be re-cloned.

## Environment constraints

- Container capped at 2 GB RAM with ~1.3 GB already used. Build with `-j2`: `cargo test -j2`.
- Nothing has been built from Scala; the JVM build was declined as unnecessary. The Scala tests are the spec.
- Commit trailer used so far: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
  Committer identity was passed per-commit with `git -c user.name=... -c user.email=...` (nothing in git config).
- rustfmt defaults (4 spaces), not the Scala repo's tabs. Run `cargo fmt` and `cargo clippy --all-targets`; both are clean now.

## Status

| Milestone | State |
|---|---|
| M0 scaffold: `FastBitSet`, `CertainMapEntry`, `Frac`, logger, util | done |
| M1 core model: everything in `src/basics/` | done; 60 tests pass |
| M2 networking & tooling (bot, command, settings, console, replay, selfPlay) | not started |
| M3 endgame solver (+ `Game::analyze`) | not started |
| M4 Reactor | not started |
| M5 Referential Sieve | not started |
| M6a–d H-Group | not started |
| M7 parity & polish | not started |

### Open decision (waiting on the user)

I proposed swapping the order to **M3 → M4 → M2**: the bot loop and self-play can't be exercised without a
real convention, and Reactor gives real behavioural tests early. The user had not answered when this file was
written. If they've answered since, follow that; otherwise ask once, and default to M3 → M4 → M2.

## Architecture (as built in M1)

- One concrete `Game` (`src/basics/game.rs`) holds everything the Scala `Game` trait declared.
- A convention is `Arc<dyn Convention>` (the old `GameOps` hooks plus static config such as the H-Group level).
  Per-game convention data goes in the `Ext` enum, which currently only has `None`; add `Reactor(..)`,
  `RefSieve(..)`, `HGroup(..)` variants as each convention lands. `Convention::new_ext` builds it for a fresh game.
- Hooks: `interpret_clue/discard/play(&self, prev: &Game, game: &mut Game, action)`, `update_turn`, `take_action`,
  `clean_hypo`, `refresh_after_play`, `find_all_clues/discards`, `eval_action`, `filter_playables`, `valid_arr`,
  `inject_replay`, endgame preferences. `game.good_touch()` etc. dispatch through `game.conv`.
- Mutation is in place (`&mut`); clones are explicit where Scala kept the old value. `handle_action` returns
  `Result<(), GameError>`; hypothetical simulation `expect`s success.
- Unknown values are `Option`, never `-1`. Players are `usize`; the common perspective is
  `Player { is_common: true, player_index: usize::MAX }`.
- `Variant::init(variants_json, suits_json)` takes the JSON as strings; the HTTP fetch is M2's job
  (URLs are `variant::VARIANTS_URL` / `COLOURS_URL`).

### Gotchas that cost time or will again

- Scala `maxBy` returns the **first** maximum; Rust `max_by_key` the last. Use `util::first_max_by_key`.
- Several Scala `FastBitSet` methods (`toList`, `map`, `flatMap`, `withFilter`) iterate **descending**; Rust's
  iterator is ascending. Where order can matter, use `.iter().rev()` (see `playables_result`).
- Scala `foldRight` + prepend preserves list order; a forward Rust loop with `push` does too, but a
  `foldRight` that *accumulates state* must iterate in reverse (see `bad_touch_result`).
- Scala `case class` `copy` calls that look pure sometimes mutate a shared `Array` (the certain map), so
  in-place mutation in Rust is equivalent only because `generate_maps` builds fresh arrays each elim.
- `Status` and `Strike` actions are deliberately not recorded in `action_list` (matches Scala).
- Hypothetical code sets the log level to Off; use `logger::LevelGuard` (RAII) rather than manual save/restore.

## Testing approach

- Port each Scala test file 1:1 next to its module; a milestone is done when its ported tests pass.
- `tests/common/mod.rs` ports `test/util.scala` + `exAsserts.scala` (`setup`, `take_turn`, `take_turn_draw`,
  `parse_action`, `pre_clue`, `fully_known`, `has_poss`, `has_infs`, `has_status`). `setup` takes an
  `Arc<dyn Convention>`; once Reactor/H-Group exist, pass them instead of `neutral()`.
- `empathy.rs` and `variants.rs` currently run against the `Neutral` convention (no interpretation).
  **Re-point them at Reactor (and the two H-Group ones at `HGroup::at_level(10)`) when those land**, and make
  sure they still pass.
- Not yet exercised by any test (compiled only): `sarcastic.rs`, `fix.rs`, `clue_result.rs`, `eval.rs`,
  `Game::navigate`. A bug in these will surface in M4/M6; check them first when a convention test misbehaves.

## Suggested next steps

1. **M3 endgame** (`src/scala_bot/endgame/{solver,winnable,helper}.scala`, ~1k lines; tests in
   `src/test/endgame.scala`, 360 lines). Note the solver uses Scala parallel collections and `Frac`;
   `winnable.scala:142` calls `withDiscard(id, target)` with a possibly invalid `target`, so check whether
   `State::add_discard` needs to accept "no order". Then port `Game::analyze` (in `basics/Game.scala`).
2. **M4 Reactor** (`src/scala_bot/reactor/*.scala`, ~2.2k lines; tests in `src/test/reactor/`, 9 files).
   `/workspace/rust-bot/src/reactor*` is a useful cross-check but may be out of date; Scala wins.
   Add `Ext::Reactor(ReactorData)` for `waiting`, `zcs_turn`, etc. (see the `Reactor` case class fields).
3. **M2** after that: `bot.scala`, `command.scala`, `settings.scala`, `console.scala`, `replay.scala`,
   `selfPlay.scala`, `analyze.scala`. rust-bot's `websocket.rs`/`command.rs` are a starting point (tokio +
   tokio-tungstenite + reqwest + serde_json). Logging into hanab.live needs the user's bot account (`.env`);
   ask before writing anything that talks to the network.
4. **M5, M6** per `PLAN.md`. H-Group is the long pole: `hgroup/hgroup.scala` alone is 1.2k lines.

At every milestone: update this file, `PLAN.md`, `PORTING_NOTES.md`; run `cargo fmt`, `cargo clippy --all-targets`,
`cargo test -j2`; commit; then summarise and **wait for the user's go-ahead** before the next milestone.
Check in mid-milestone for M6a (the other large one) and whenever a real design question comes up.

## Licence

The Scala repo is GPLv3, so this port is a derivative work and stays GPLv3 (`LICENSE` copied in, `Cargo.toml` says so).
