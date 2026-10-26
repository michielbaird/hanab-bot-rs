# Porting notes

Where the Rust differs from the Scala, and why. Update this as the port proceeds.

## Layout

| Scala | Rust |
|---|---|
| `lib/FastBitSet.scala` | `src/collections/fast_bit_set.rs` |
| `lib/CertainMapEntry.scala` | `src/collections/certain_map_entry.rs` |
| `lib/fraction.scala` | `src/frac.rs` |
| `lib/IdentitySet.scala` | moves to M1 (depends on `Identity` and `State`) |
| `logger.scala` | `src/logger.rs` |
| `util.scala` | `src/util.rs` (+ `std` iterators, below) |
| `test/lib/CertainMap.scala` | `tests/certain_map.rs` |

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
