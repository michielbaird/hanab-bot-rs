# Handoff: scala-bot → Rust rewrite

Written at the end of M4 so another session can pick this up cold. Read this, then `PLAN.md` (milestones)
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
| The new crate (all work happens here) | `/workspace/hanab-bot-rs` (git repo; see `git log`: M0, M1, M3, M4 are separate commits) |
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
| M1 core model: everything in `src/basics/` | done |
| M2 networking & tooling | done: `self_play`, `replay`, `analyze`, `bot` binaries. One live game played (HGroup2, won) |
| M3 endgame solver (+ `Game::analyze`) | done |
| M4 Reactor | done (93 ported tests) |
| M5 Referential Sieve | done (87 ported tests) |
| M6 H-Group (levels 1-12) | done: all 453 `test/hgroup/**` tests ported and green (`tests/hgroup/`); `tests/empathy.rs` and the H-Group rainbow endgame test run on H-Group |
| M7 parity & polish | mostly done: Scala parity measured, crash sweep clean, performance work (see below). Open items at the end of this file |

All tests pass (`cargo test -j2`); `cargo fmt` is clean.

### Decision

Order followed: M3, M4, M2, M5, M6, then M7. Scala oddities are noted in `FIX_LATER.md`. The port is now finished, so those can be fixed (the bot is diverging from Scala on purpose).

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
- `empathy.rs`, `variants.rs` and `endgame.rs` run on Reactor. 
- Exercised only through Reactor so far: `sarcastic.rs`, `fix.rs`, `clue_result.rs`, `eval.rs`. Still untested: `Game::navigate`, `Game::analyze` (M2 tooling will cover them). A bug in these will surface in M4/M6; check them first when a convention test misbehaves.

## Credentials

The user gave a test account for hanab.live (`michiel-test`; password in the untracked `.env` as `HANABI_USERNAME0` / `HANABI_PASSWORD0`). `.env` and `seeds/` are gitignored. Don't commit or print the password. **Ask the user before every use of these credentials** (running `bot`, any login or live connection); a yes covers that one use only (the user said yes once for a live session on the day M7 started; that does not carry over). The account can't play alone: a human (or a second account the user approves) must start a table and `/join` + `/start` the bot by PM.

## Suggested next steps

1. **Open performance items** (see `FIX_LATER.md` section C): `Game::elim` is still ~55% of self-play time, mostly the per-player
   `update_hypo_stacks` (`hypo_play` + `refresh_wcs` when connections are waiting). RefSieve and H-Group at 5 players use a lookahead
   cap that Scala does not have (`MAX_LOOKAHEAD_5P`); the Scala bot cannot finish 5p H-Group games at all.
2. **RefSieve at 5 players** averages only ~18.7 with the capped lookahead (6 games). Not compared with Scala (it does not finish).
   RefSieve on Pink and Muddy Rainbow averaged ~20 in 3-game samples; worth a larger sample.
3. **More live games** (HGroup12, Reactor1, RefSieve; higher log level). One HGroup2 game has been won live. Ask before logging in.
4. **`benchmark` tool** (`benchmark.scala`) is not ported.
5. Open correctness investigation: none known. HGroup5 seed 9 (`investigations/`) is behaviour the Scala bot shares.
6. Work through the GitHub issues (<https://github.com/michielbaird/hanab-bot-rs/issues>; `FIX_LATER.md` has the background for each): section A lists the open Scala oddities, B robustness (item 34), C what is left of the performance work.

Useful commands (release build, `-j2`):

- `self_play games=10 seed=0 players=3 variant="No Variant" convention=HGroup12 threads=2` (variants other than No Variant / 6 Suits need
  `variants=<variants.json> colours=<suits.json>`, see `VARIANTS_URL` / `COLOURS_URL` in `src/basics/variant.rs`). `SELF_PLAY_SLOW_MS=<ms>`
  prints slow decisions. Seeds do not reproduce the Scala bot's shuffles.
- `analyze file=seeds/N.json convention=...` and `replay file=... index=N` for exported games.
- Scala build for comparison: `JAVA_HOME=~/tools/jdk scala-cli run /workspace/scala-bot --server=false --java-opt=-Xmx1200m
  --main-class scala_bot.selfPlay -- games=... convention=...` (also `scala_bot.analyze`). Takes ~10 minutes to compile the first time.
- `cargo test --test clone_probe -- --ignored --nocapture` prints allocations and time per `Game` part.

At every milestone: update this file, `PLAN.md`, `PORTING_NOTES.md`; run `cargo fmt`, `cargo clippy --all-targets`, `cargo test -j2`; commit.

## Licence

The Scala repo is GPLv3, so this port is a derivative work and stays GPLv3 (`LICENSE` copied in, `Cargo.toml` says so).

## Scala parity (measured)

Scala bot built with scala-cli + JDK 25 (`--server=false --java-opt=-Xmx1200m`); decks differ between the implementations
(different PRNGs), so these are statistical comparisons over 30 games, 3 players, No Variant:

| Convention | Rust avg / perfect | Scala avg / perfect |
|---|---|---|
| HGroup5 (seeds 100-129) | 24.43 / 22 | 24.33 / 22 |
| RefSieve (seeds 200-229) | 23.67 / 17 (1 strikeout) | 24.37 / 17 |

Rust HGroup12 (10 games): 24.3 avg, 6 perfect. Scala `analyze` agrees with every move of the Rust HGroup5 seed-9 strikeout game (FIX_LATER 28).

## M7 sweep and performance (measured on an idle machine)

- Crash sweep (no panics or cut-short games anywhere): HGroup1-12 at 2-4 players (3 games each), RefSieve and Reactor1 at 2-5 players,
  and HGroup12 / RefSieve / Reactor1 on Rainbow, Black, White, Pink, Brown, Prism, Muddy Rainbow, Orange and 6 Suits (3 players, 3 games each;
  needs `variants=<variants.json> colours=<suits.json>`). Only failures: H-Group at 5 players (fixed, FIX_LATER 29) and
  `RefSieve players=5`, which still does not finish within 15 minutes (same lookahead blow-up, not yet capped).
- Speed, HGroup5 3p self-play: one thread plays a game in about 10.7 s (51 s before the M7 optimizations). 4 games on 4 threads: 70 s wall
  (Rust) vs 616 s wall (Scala, different decks, includes JVM start-up), roughly 9x.

## Verified live (2026-10-27)

Deployed by the user from the Docker image on a NAS (PUID/PGID set): two bots on this Rust port and one Scala bot side by side, the
multi-account host (`bot index=...`), the game-start `/feedback` announcement and the `/feedback` command (entries appear in `feedback.jsonl`)
all work. Still unwatched: how the bots behave over days of real games (use `analyze_history`, see README) and the `kind: error` fallback path
(only tested without a network).
