# Performance

How to measure, what the numbers were, and what is left. Issue numbers are on GitHub.

## Measuring

```
cargo build --release --bin benchmark
./target/release/benchmark                        # HGroup3, 3 players, 3 games from seed 314, 3 repetitions
./target/release/benchmark convention=Reactor1 players=4 games=2
CARGO_TARGET_DIR=/tmp/target-stats cargo run --release --features stats --bin benchmark   # also counts the work done
```

Every repetition plays the same seeded games, so the times can be compared between runs and between versions. There is no warm-up (no JIT or GC):
the first repetition is as fast as the rest. Run it on an otherwise idle machine and compare the minimum. The endgame solver is off by default
(`solver=on` to include it) because its time limit makes it depend on the machine.

Two numbers do not depend on the machine:

* **fingerprint**: a hash of every move of every game. A change that is meant to leave the bot's behaviour alone must leave it unchanged;
  a change that alters play changes it (then compare scores, not just times).
* **work counters** (`--features stats`): how many times the expensive functions ran per repetition. They compile to nothing without the
  feature and do not change behaviour (the fingerprint is the same with and without).

## Baseline

HGroup3, 3 players, 3 games from seed 314, solver off (commit of the benchmark tool):

| | per repetition |
|---|---|
| time (this development machine) | about 36.0 s |
| fingerprint | `25bf41aa68672330` |
| total score of the 3 games | 70 (170 moves, 167 decisions) |
| decisions (`take_action`) | 167 |
| `Game::elim` | 300,090 |
| `update_hypo_stacks` | 1,158,712 |
| hypothetical plays | 2,253,016 |
| simulated games (`simulate_clue` / `simulate_action`) | 106,174 |

That is about 636 simulated games and 6,900 `update_hypo_stacks` per decision.

## History

Self-play of 2 HGroup5 games, one thread: about 102 s at the start of M7, 52 s after skipping the game copies in hypothetical plays when nothing is waiting
(`uses_refresh_after_play`), 27 s after making `Game::clone` cheap (copy-on-write `Ext`, action list and notes), and about 22 s after skipping the inverted-suit
trash scan in `update_hypo_stacks` (all on a machine that was sometimes busy). About 9x faster than the Scala bot and about 50x less memory.

## Where the time goes

`Game::elim` is about 55% of self-play time, and `update_hypo_stacks` about 75% of that: `elim` runs it for the common perspective and once for
every player, each time replaying the hypothetical plays. Ideas, roughly by expected gain (see issues #14, #15):

1. Not computing a perspective's hypo stacks when it cannot differ from the common one.
2. Fewer game copies in hypothetical plays when connections are waiting (the `prev` copy and `refresh_wcs`).
3. Caching `order_trash` / `thinks_trash`, which are expensive and called from many places.
4. Searching the endgame in parallel (the solver is single-threaded and often hits its time limit).
