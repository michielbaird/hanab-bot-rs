# scala-bot → Rust rewrite plan

Source: `/workspace/scala-bot` (v1.2.8, ~16.6k lines source, ~12.7k lines tests, GPLv3)
Reference: `/workspace/rust-bot` (the author's earlier Rust port, Reactor only, ~9k lines, abandoned at v1.0.2)
Target: `/workspace/scala-bot-rs` (new Cargo crate, edition 2024, rustc 1.99)

## Size of the job

| Scala module | Lines | Notes |
|---|---|---|
| lib (FastBitSet, IdentitySet, CertainMapEntry, Frac) | 688 | bit-packed helpers, mostly mechanical |
| basics (Game, State, Player, Card, Variant, Action, elim...) | 4170 | core data model, everything depends on it |
| endgame (solver, winnable, helper) | 1001 | search; performance-sensitive |
| reactor | 2236 | rust-bot already has an older port of this |
| refSieve | 1499 | |
| hgroup | 5412 | biggest and most intricate (levels 1-12) |
| top level (bot, command, replay, selfPlay, analyze, util...) | 1600 | networking, commands, tooling |
| tests | 12665 | the spec; ported alongside each module |

## Approach

- **Tests are the spec.** The Scala tests encode expected bot behaviour. Each module is ported together with its tests, and a milestone is done only when its ported tests pass. Tests are ported 1:1, not rewritten.
- **Idiomatic Rust, same behaviour.** Keep module boundaries recognisable so each Rust file maps to a Scala file, but use Rust idioms from the start (`Option`/`Result`, iterators, enums, ownership instead of copy-everything, `std` traits). Any change that could alter behaviour is recorded in `PORTING_NOTES.md`.
- **Data model.** Scala's `Game` is an immutable value updated via `copy`/`GameUpdates`. In Rust I'll use plain structs with `Clone`, and `Vec`s of small `Copy` types (bitsets, `u8` indices), cloning at the same points Scala copies (hypotheticals, rewinds, endgame search). No `Rc<RefCell>` graphs.
- **Convention dispatch.** Scala uses a `Game` trait with three implementations (HGroup, RefSieve, Reactor). In Rust: a `Convention` trait, with the shared state in one struct and per-convention data (`ConvData`) as an enum.
- **Reuse from rust-bot** where it matches the Scala logic: variant/identity-set/card basics, websocket + command plumbing, reactor, endgame. It is older than scala-bot, so it's a head start, not a source of truth; Scala wins on any disagreement.
- **Dependencies:** tokio, tokio-tungstenite, reqwest, serde/serde_json, regex, rand/rand_chacha, itertools, ahash, dotenvy (same set rust-bot used).
- **Memory:** the container is capped at 2 GB with ~1.3 GB already in use, so I'll build with `-j2`, and debug profile for tests unless the endgame solver needs `--release`.

## Milestones

Each milestone ends with a checkpoint: what's done, test pass counts, deviations from Scala I had to make, and open questions for you. I stop and wait for your go-ahead at each one.

| # | Milestone | Scala source | Done when |
|---|---|---|---|
| M0 | Scaffold | lib/ (minus IdentitySet), logger, util | crate builds; FastBitSet, CertainMapEntry, Frac, logger, util tested. **Done.** |
| M1 | Core model (incl. IdentitySet) | basics/, lib/IdentitySet | Variant, Card, Clue, State, Action, Player, elim, Game compile; top-level `empathy` and `variants` tests pass. **Done** (`analyze` deferred to M3) |
| M2 | Networking & tooling | bot, command, settings, console, replay, analyze, selfPlay | bot logs into hanab.live (needs your account/.env), joins a table and plays legal moves with a stub convention; replay + self-play binaries run |
| M3 | Endgame | endgame/ | `endgame` tests pass; solver speed within reasonable range of Scala |
| M4 | Reactor | reactor/ | `test/reactor/*` pass |
| M5 | Referential Sieve | refSieve/ | `test/refSieve/*` pass (incl. 2-player) |
| M6a | H-Group core (L1-4) | hgroup: hgroup, interpretClue, connect, connHelper | `test/hgroup/level1`, `level2*`, `level3`, `level4` pass |
| M6b | H-Group stalling, Occam, WCs (L5-8) | stalling, occams, updateWcs | `level5*`, `level6`-`level8` pass |
| M6c | H-Group CM/DC/special clues (L9-12) | interpretCm, interpretDc, specialClues | `level9`-`level11*` pass |
| M6d | H-Group variants | stateEval + variant tests | `test/hgroup/variants/*` pass |
| M7 | Parity & polish | all | full test suite green; self-play games complete without panics for all three conventions; README updated |

Check-ins also happen mid-milestone for M1 and M6a (the two largest), and whenever I hit a real design question.

## Verification

- Ported tests per milestone (primary).
- Self-play across conventions and player counts, looking for panics, illegal moves and stalls.
- Optional: build the Scala bot (needs JDK 25 + scala-cli, which are already in `~/tools`, memory permitting) and compare decisions on the same seeded self-play games. This is the only way to catch behaviour that no test covers. It is not required for the milestones above.

## Risks

- **Scale:** M6 alone is ~5.4k lines of dense convention logic. Expect it to take most of the effort.
- **Hidden behaviour:** Scala-specific semantics (immutable sharing, lazy collections, `Option` chains, parallel collections in the solver) can change behaviour subtly if ported loosely.
- **Licence:** the repo is GPLv3, so the Rust port is a derivative work and stays GPLv3 (LICENSE and attribution carried over).
- **Maintenance:** the author abandoned the earlier Rust rewrite in favour of Scala. This port will be unofficial and not upstream unless they want it.
