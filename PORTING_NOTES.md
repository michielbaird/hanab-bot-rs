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
- `Game::analyze` is not ported yet; it needs the endgame solver (M3).

### Test coverage note

`empathy` and `variants` were written against Reactor/H-Group in Scala. Until those exist they run
against `tests/common::Neutral` (no interpretation, just empathy), which is enough for their
assertions about possibilities. Re-point them at the real conventions in M4/M6.
