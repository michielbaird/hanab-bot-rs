# Porting notes

Where the Rust differs from the Scala, and why. Update this as the port proceeds.

## Layout

| Scala | Rust |
|---|---|
| `lib/FastBitSet.scala` | `src/collections/fast_bit_set.rs` |
| `lib/CertainMapEntry.scala` | `src/collections/certain_map_entry.rs` |
| `lib/fraction.scala` | `src/frac.rs` |
| `lib/IdentitySet.scala` | `src/basics/identity_set.rs` |
| `logger.scala` | `src/logger.rs` |
| `util.scala` | `src/util.rs` (+ `std` iterators, below) |
| `test/lib/CertainMap.scala` | `tests/certain_map.rs` |
| `basics/Variant.scala` | `src/basics/variant.rs` |
| `basics/Card.scala` | `src/basics/card.rs` (`Identity`, `Card`, `Thought`, `ConvData`, `CardStatus`) |
| `basics/clue.scala` | `src/basics/clue.rs` |
| `basics/State.scala` | `src/basics/state.rs` (also `TableOptions`, from `command.scala`) |
| `basics/Action.scala` | `src/basics/action.rs` |
| `basics/Connection.scala` | `src/basics/connection.rs` |
| `basics/Player.scala` | `src/basics/player.rs` |
| `basics/playerElim.scala` | `src/basics/player/elim.rs` |
| `basics/Game.scala`, `basics/basics.scala` | `src/basics/game.rs`, `game/update.rs`, `game/replay.rs` |
| `basics/clueResult.scala`, `eval.scala`, `fix.scala`, `sarcastic.scala` | same names in `src/basics/` |
| `utils.visibleFind` | `util::visible_find` |
| `test/util.scala`, `test/exAsserts.scala` | `tests/common/mod.rs` |
| `test/empathy.scala`, `test/variants.scala` | `tests/empathy.rs`, `tests/variants.rs` |
| `endgame/{solver,winnable,helper}.scala` | `src/endgame/{solver,winnable,helper}.rs` |
| `Game.analyze` | `src/basics/game/analyze.rs` |
| `test/endgame.scala` | `tests/endgame.rs` |
| `reactor/reactor.scala` | `src/reactor.rs` (`Reactor` convention, `ReactorGame` trait, `take_action`, `update_turn`, ...) |
| `reactor/interpretClue.scala` | `src/reactor/interpret_clue.rs` |
| `reactor/interpretReaction.scala` | `src/reactor/interpret_reaction.rs` |
| `reactor/interpretReactive.scala` | `src/reactor/interpret_reactive.rs` |
| `reactor/stateEval.scala` | `src/reactor/state_eval.rs` |
| `test/reactor/*.scala` | `tests/reactor/*.rs` (one test binary) |
| `refSieve/refSieve.scala` | `src/ref_sieve.rs` (`RefSieve` convention, `RefSieveGame` trait) |
| `refSieve/interpretClue.scala` | `src/ref_sieve/interpret_clue.rs` (+ `interpret_clue_impl`, the body of `RefSieve.interpretClue`) |
| `refSieve/connect.scala` | `src/ref_sieve/connect.rs` |
| `refSieve/stateEval.scala` | `src/ref_sieve/state_eval.rs` |
| `test/refSieve/**` | `tests/ref_sieve/*.rs` (one test binary; mechanically converted, then checked) |

## Deliberate deviations

- **`FastBitSet` iteration order.** Scala's `toList`, `map`, `flatMap` and `withFilter` return
  *descending* order; `iter` and `foreach` are ascending. Rust always iterates ascending.
  When porting a call site that used one of the descending methods, use `.iter().rev()`.
- **`FastBitSet::head` / `max`** throw on empty in Scala; Rust has `first()` / `last()` returning `Option`.
- **`CertainMapEntry` is `u64`, not `Int`.** Scala packed 3 slots into 32 bits and silently
  corrupted on a 4th. Rust holds up to 6 and panics beyond that.
- **`CertainMapEntry::contains`** is split into `contains_order(order)` and
  `contains(order, unknown_to)`. The latter keeps Scala's `order == o || u == unknownTo`
  disjunction. `unknownTo = -1` ("known to everyone") is `None`.
- **`Frac`** is normalised (lowest terms, positive denominator) on construction, so `==` and
  `Hash` are the derived ones. Comparison cross-multiplies in `i128`.
- **Logging** has no runtime-vs-compile-time split to port by hand: the `verbose-logs` feature
  replaces Scala's `compileTimePriority`. Macros are `log_error!`, `log_warn!`, `log_info!`,
  `log_highlight!`.
- **`players_until`** is capped at `num_players` items (Scala would loop forever if `end` were out of range).

## `utils.scala` → `std`

| Scala | Rust |
|---|---|
| `loop`, `loopIf` | `for` / ranges |
| `it.fastForeach/Forall/Exists/Count` | `for`, `all`, `any`, `filter().count()` |
| `foldLeftOpt` | `try_fold` |
| `findSome` | `find_map` |
| `summing` | `.map(f).sum()` |
| `maximizing(default)(f)` | `.map(f).max().unwrap_or(default)` (use `fold` for floats) |
| `minimizing(default)(f)` | `.map(f).min().unwrap_or(default)` |
| `existsM(pf)` | `any(\|x\| matches!(x, ...))` |
| `cond` / `when` / `pipe` / `tap` | `if` / `Option::then`, plain calls |
| `visibleFind` | M1 (needs `State`, `Player`) |

## M1: the core model

### Shape

- **No `GameOps` / `GameUpdates` / `copyWith`.** One concrete `Game` struct holds everything the Scala
  `Game` trait declared. A convention is an `Arc<dyn Convention>` (the old `GameOps` hooks, plus static
  config like the H-Group level), and its per-game data goes in the `Ext` enum (variants are added as
  conventions are ported). Hooks take `prev: &Game` and `game: &mut Game`.
- **Mutation, not copying.** Functions that returned a modified `Game`/`Player`/`State` now take `&mut`.
  Where Scala keeps the old value around (`prev`, hypotheticals, rewinds), Rust clones explicitly
  (`handle_action` clones once per clue/play/discard to give the convention a `prev`).
- **`elim` derives every perspective from the same starting game**, as Scala did: each player is
  cloned, updated against the pre-elim `self`, and only then are the results written back.
- **Unknown values are `Option`, not `-1`.** `Card::identity`, `Thought::seen`, action ids,
  `exclude_order` in `is_duped`/`is_trash`, `except` in `elim`, `unknown_to` in `CertainMapEntry`.
  `Thought::partial_id` returns a `PartialId` for the one Scala caller that used `partial = true`.
- **`Variant`**: `colourable_suits` holds suit *indices*; `State` holds an `Arc<Variant>`; regexes of
  alternated literals became `NamePattern`. `Variant::init` takes the two JSON documents as strings
  (the HTTP fetch belongs to the networking layer, M2).
- **`IdentitySetOpt`** is a thin wrapper over `Option<IdentitySet>`.
- **Errors**: `Game::handle_action` returns `Result<(), GameError>` (bad data from the server);
  `rewind`/`replay` return `Result<_, String>` as in Scala. Hypothetical simulation `expect`s success.
  Broken invariants the Scala threw `Error`/`AssertionError` for are panics.
- **Common perspective** is `Player { is_common: true, player_index: usize::MAX }`, so indexing by it fails loudly.

### Behaviour that differs or was easy to get wrong

- `endgame_turns` is `Option<isize>`; `clue_tokens` and `cards_left` saturate at 0 rather than going negative.
- A `TurnAction` with no current player (game over) leaves `state.current_player_index` as it was.
- `Status` and `Strike` actions are not recorded in `action_list` (Scala fell through to `case _ => game`).
- Scala's `maxBy` returns the first maximum, Rust's `max_by_key` the last; `util::first_max_by_key` keeps Scala's.
- Scala's `FastBitSet.foreach` is ascending but several `List`-building methods (and
  `playablesResult`'s outputs) are descending; those call sites keep the Scala order.
- `ConvData::reason` compares against the *oldest* reasoning entry (`lastOption` on a prepended list), as Scala does.
  It looks unintended, but changing it could change behaviour.
- `take_action` is synchronous (Scala returned `IO`); revisit in M2 if the bot needs it async.

### Test coverage note

`empathy` and `variants` were written against Reactor/H-Group in Scala. Until those exist they run
against `tests/common::Neutral` (no interpretation, just empathy), which is enough for their
assertions about possibilities. Re-point them at the real conventions in M4/M6.

## Endgame solver (M3)

- **`RemainingMap` is a `[u8; 30]` indexed by identity ordinal** (`Copy`, cheap to hash), not an immutable `Map`.
  Scala's `Map` iterates in insertion order up to 4 entries and by hash after that, so tie-breaks (which of
  several equally good actions or arrangements comes first, which trash id counts as "the" trash id) were
  unspecified there. Rust iterates by ordinal, so results are deterministic. Winrates are unaffected.
- **No generic `EndgameSolver[G]`.** `Game` is concrete, so `EndgameSolver` isn't generic; `winnable_if`,
  `winnable_simpler`, `clueless_winnable` and `advance_state` are plain functions on `State`.
- **Arrangements are expanded eagerly**, layer by layer (Scala used lazy iterators). Same set, same order,
  same deadline checks before each expansion.
- **Monte Carlo grouping** keeps first-seen order (Scala's `Map.valuesIterator` order is unspecified) before the
  stable sort by probability.
- **Log level** is restored by a `LevelGuard` on every return path. The Scala solver left logging off when it
  returned from the `onlyAction` branch.
- **Defensive deviations** where Scala would throw: `optimize` returning an empty action list on timeout is
  reported by `solve` as `timeout` instead of calling `.head` on it; `unwinnable_state` with an empty
  `remaining` skips the bottom-deck check; the "winning play" search in `winnable` ignores plays with an
  unknown id instead of calling `.get`.
- **Faithfully kept oddities** (tracked in `FIX_LATER.md`): `optimize` doesn't reduce `rem_prob` after a *winnable* arrangement (only
  after a pre-filtered one); the pruning bound in the multi-arrangement loop starts at `1 - winrate` instead
  of `1 - first_prob`; the drawn `Card` in `winnable_if` has order `next_card_order + 1` though it is stored at
  `next_card_order`; `Game::analyze` calls the solver on the final game, not the replayed one. All look
  unintended, but none changes a result in the tests, and changing them would change behaviour.
- **Parallel collections**: the Scala solver doesn't use any (the handoff note was wrong); it is single-threaded.
- `thinks_playables` has `assume = true` by default in Scala; the solver passes `true` explicitly.

### Test coverage note (M3)

`test/endgame.scala` ran on Reactor (and once H-Group). `tests/endgame.rs` runs on `Neutral`, extended to offer every
valid clue and to discard the oldest card. 8 of 10 ported tests pass. `it_completes_an_early_endgame` and
`ends_an_endgame_as_quickly_as_possible` are `#[ignore]`d: they need clues to teach players what to play, which
only a real convention does. The H-Group variant of the rainbow test was ported in M6 (`it_wins_an_unconventional_rainbow_endgame_in_h`).

## Reactor (M4)

- **State.** Reactor-only fields (`waiting`, `zcs_turn`) live in `Ext::Reactor(ReactorData)`. `Reactor` is a unit struct
  implementing `Convention`; `ReactorGame` is an extension trait on `Game` (`waiting()`, `zcs_turn()`, `chop()`,
  `must_clue()`, `must_clue_as(player)`). `must_clue_as` replaces Scala's `game.withState(_.copy(currentPlayerIndex = ..)).mustClue`.
- **Hooks mutate in place.** `interpret_clue/discard/play` write into `&mut Game`. Internally the interpretation helpers
  (`interpret_stable`, `try_stable`, `interpret_reactive*`, `react_play/discard`, `target_play/discard`, ...) are
  functional: they take `&Game` and return a new `Game`, because the Scala falls back to the original `game` in several
  branches (`TryReactive`, `Mistake`).
- **`take_action` is synchronous** (Scala returned `IO` only to run the endgame solver on a blocking thread).
- **Meta/thought updates** use `common.update_thought(order, ..)` (marks dirty, like `withThought`) and direct writes to `game.meta[order]`.
- **`delayed_plays`** builds its list the way Scala's prepending fold does (later entries first; `IdentitySet.map` is descending),
  since `target_play` takes the *first* match.
- **`maxBy` ties** are resolved to the first maximum throughout (`take_action`, `find_all_clues`).
- Oddities that are kept on purpose are in `FIX_LATER.md` (items 8-16).
- **New hook** `Convention::uses_refresh_after_play` (default `false`): avoids a game copy per hypothetical play.

### Test coverage note (M4)

All of `test/reactor/*.scala` (93 tests, 9 files) is ported and passes. `tests/empathy.rs` (the five Reactor tests),
`tests/variants.rs` and `tests/endgame.rs` now run on Reactor. The two H-Group empathy tests still run on `Neutral`, and the
H-Group endgame test is not ported (M6). `tests/endgame.rs` gives the solver a 60s timeout (Scala's tests use the default 3s):
`it_wins_an_unconventional_rainbow_endgame` and `it_completes_an_early_endgame` need ~2.6s and >3s respectively in a release build.

## Tooling (M2, offline part)

- `settings.scala` -> `src/settings.rs` (`ConventionKind` + `Display`/`parse`, `info_note`). `ConventionKind::build()` fails for
  RefSieve/HGroup until they are ported. The Scala convention patterns are unanchored (a string merely *containing* `rs`
  is RefSieve); that is kept.
- `bot.scala`'s `parseArgs`/`readEnv` -> `tools/args.rs`. `selfPlay.scala` -> `tools/self_play.rs` + `src/bin/self_play.rs`;
  `replay.scala`/`analyze.scala` -> `tools/game_data.rs` (+ `bin/replay.rs`, `bin/analyze.rs`); `console.scala` -> `tools/console.rs`.
- **Seeds do not match Scala's.** The shuffle is a small in-crate SplitMix64 (`Rng`), not `scala.util.Random`.
- **Variants offline.** `load_variant` has built-ins for "No Variant" and "6 Suits"; anything else needs
  `variants=<variants.json> colours=<colors.json>` (the documents behind `variant::VARIANTS_URL` / `COLOURS_URL`).
- **Self-play catches panics** per move (`catch_unwind`) and reports them in `GameSummary::error`, ending the game like the Scala
  bot's `try/catch` does (`endgameTurns = Some(0)`). Threads default to 2 (`threads=`) because of the memory cap.
- `GameData::fetch_id` (download by id) is not implemented: it needs the network.

## Client (M2, networked part)

- `command.scala`'s `BotClient` -> `src/client.rs`, sans-IO: `handle_msg(&str)` pushes `Out::Send(String)` / `Out::Wait(Duration)`
  onto a tokio unbounded channel (so analysis threads can reply later). `bin/bot.rs` owns the websocket: it spaces sends by 500ms,
  pings every 30s, times out after 2 minutes of silence, logs in again and retries up to 5 times with `2 << attempt` second backoff.
  `BotClient` runs on its own OS thread because choosing an action can take seconds.
- The Scala `IO.sleep(2.seconds)` race is `Out::Wait(2s - time spent choosing)` before the `action` send (no wait in `/fastmode`).
- Panics inside message handling and action choosing are caught and logged (the message is dropped); this is the answer to
  FIX_LATER item 14 for the live bot.
- `/settings`/`/setall` refuse conventions that `ConventionKind::build` can't construct yet (RefSieve, H-Group) instead of accepting them (all three conventions are now buildable).
- TLS is rustls (not the JVM's); HTTP uses blocking `reqwest` (login, variants, `/export/<id>`), so `GameData::fetch_id` and `replay id=...` work.
- Not verified live: a complete game (needs a second player). Login, websocket and the lobby handshake were exercised against hanab.live.

## Referential Sieve (M5)

- **State.** `Ext::RefSieve(RefSieveData { waiting, zcs_turn, locked_shifts })`. `waiting` is only ever appended to (nothing reads it yet,
  as in Scala). `RefSieveGame` (extension trait on `Game`) has `chop`, `must_clue(player)`, `find_finesse`, `check_2p_ptd`, `unlock_promise`.
- **Connections** use the shared `basics::connection::Connection` enum: Scala's `KnownConn`/`PlayableConn`/`PromptConn`/`FinesseConn`
  map to `Known`/`Playable`/`Prompt`/`Finesse` with the defaulted fields filled in.
- **Ordering.** `target_play` walks the inferences in *descending* order (Scala's `withFilter` on an `IdentitySet` prepends).
- `find_connecting`: a candidate prompt that turns out wrong yields `None` and does *not* fall back to a finesse (Scala's
  `Option[Option[_]].getOrElse`). Kept.
- **Tests were converted by script** (`/tmp` helper, not committed): hands, options and `takeTurn`/`hasInfs`/`hasStatus`/simple asserts
  are mechanical, and the 25 non-trivial asserts were translated by hand. All 87 tests are present (counted against the Scala).
- Items kept from Scala that look unintended are in `FIX_LATER.md` (19-23).

## M6: H-Group

- `src/hgroup.rs` + `src/hgroup/*.rs` mirror `hgroup/*.scala` file for file (`interpret_clue`, `connect`, `conn_helper`, `occams`,
  `stalling`, `special_clues`, `update_wcs`, `interpret_cm`, `interpret_dc`, `state_eval`). Per-game state is `Ext::HGroup(Box<HGroupData>)`;
  convention-specific helpers are on the `HGroupGame` extension trait.
- Helpers that fall back to the original game take `&Game` and return a new `Game`; hooks assign `*game = result`.
- `SavedCtx` (the Scala closure captured by `importantFinesse`) stores `prev`, `game`, `action` and the focus, and is re-run via `as_ctx()`.
- `HGroup` returns `true` from `uses_refresh_after_play`.
- Tests: `tests/hgroup/*.rs` are 1:1 ports (453 tests). Most were converted mechanically, the rest by hand. Scala tests that build a custom
  variant use `TestVariant::{WhiteOnesDarkPrism5, LightPinkGray5}`.

## M7: performance and parity

- `Game::clone` is hot (hypothetical games everywhere), so the rarely mutated parts are `Arc`s copied on write: `Ext::HGroup(Arc<HGroupData>)`
  (`hg_mut` does `Arc::make_mut`), `State::action_list` and `Game::notes`. Mutate them through `Arc::make_mut`.
- `Convention::uses_refresh_after_play(&self, &Game)`: when false, hypothetical plays skip the `prev` copy (H-Group: only when connections are waiting).
- `update_hypo_stacks` only scans for inverted-suit trash when the variant has an inverted suit.
- 5+ players: H-Group and RefSieve `advance` stop after `MAX_LOOKAHEAD_5P = 1` plies (not in Scala, which does not finish such games).
- Memory: Rust peaks at ~8 MB (1 thread) to ~15 MB (4 threads) for HGroup5 self-play; the Scala bot peaked at ~795 MB for the same 4 games
  (JVM, `-Xmx1200m`).
- `self_play` silences the panic hook; a panicking game shows up as "(game cut short: ...)" and in "Games cut short by an error".

## Several bots in one process

- `bot index=0,1,2` runs one `supervise` task per account (login, reconnect with back-off) and one `BotClient` + decision thread per
  connection; they share only the immutable variant registry (`Variant::init`, once) and the tokio runtime. A bot that gives up does not stop the others.
- The log level is per thread (`logger::set_level`, `LevelGuard`); `logger::set_default_level` sets the process-wide default. A process-wide
  level raced between bots (one bot's temporary quieting could be restored by another).
- Live-bot logging is compiled out unless the `verbose-logs` feature is on, so the default `bot` prints little more than connection events and
  sent commands. Log lines from `BotClient` itself are not prefixed with the bot's name.
- Tested only without a network (`several_clients_in_one_process_do_not_interfere`): the multi-account path has not been run against hanab.live.
