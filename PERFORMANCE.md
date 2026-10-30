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

Benchmark (baseline above), after making the game copy in `update_hypo_stacks` cheaper: 36.0 s -> 30.1 s, fingerprint unchanged. `update_hypo_stacks` copies the whole
game for every call (about 14% of the run, 37% of `update_hypo_stacks` itself), so what a copy costs matters: the perspective being worked out is no longer copied twice,
player names are shared (`Arc<str>`, `Arc<Vec<String>>`) and the discard stacks are copy-on-write. The clone probe (`cargo test --release --test clone_probe -- --ignored --nocapture`)
shows what is left. Sharing `ConvData::reasoning` (copy-on-write) took it to 27.4 s. Making `State::deck` copy-on-write gained nothing measurable, so it was dropped. Sharing each player's `thoughts` (copy-on-write, `Player::thoughts_mut`) took it to 24.7 s. Left in a game copy: `State::hands`, `State::deck`, `meta` and the per-player maps.

Profile of the same run (inclusive timers, temporary): `Game::elim` 16.7 s of 28.7, of which `update_hypo_stacks` 12.0 (game copy 3.6, hypothetical plays 3.8, `thinks_playables` 2.4) and `card_elim` 2.4. `order_trash` is only about 0.5 s in total (1.3M calls), so caching it (idea 3) would not pay.

## Where the time goes

`Game::elim` is about 55% of self-play time, and `update_hypo_stacks` about 75% of that: `elim` runs it for the common perspective and once for
every player, each time replaying the hypothetical plays. Ideas, roughly by expected gain (see issues #14, #15):

1. Not computing a perspective's hypo stacks when it cannot differ from the common one. Tried: 92% of the per-player results do equal the common one, but there is
   no cheap test that proves it beforehand (players differ in what they see, `Thought::seen`, which `id_with` reads), so the result can't be skipped soundly.
   Copying less of the game for the hypothetical does not work either: `thinks_playables` reads the other players' views.
2. Fewer game copies in hypothetical plays when connections are waiting (the `prev` copy and `refresh_wcs`).
3. ~~Caching `order_trash` / `thinks_trash`~~: measured, about 2% of the run; not worth the invalidation risk.
4. Searching the endgame in parallel (the solver is single-threaded and often hits its time limit).
