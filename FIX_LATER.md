# Oddities and follow-ups

The port is finished and now diverges from the Scala bot on purpose ("forking mode"): anything below can be fixed, one at a
time, re-running the tests (`cargo test -j2`) and a self-play sweep after each. Item numbers are stable (other docs refer to them).

**Open items are tracked as GitHub issues** (<https://github.com/michielbaird/hanab-bot-rs/issues>); this file keeps the background. Item to issue:

| FIX_LATER item | Issue |
|---|---|
| 8 | #2 Reactor: try_stable overwrites an existing waiting connection |
| 11 | #3 Reactor: elim_dc_play / elim_play_play lose all but the last trash flag |
| 12 | #4 Reactor: calc_target_slot turns a missing index (-1) into slot 0 |
| 13 | #5 Reactor: must_clue's unknownPlay on a possibly-empty old_inferred |
| 21 | #6 RefSieve: clue evaluation scales by `mult` regardless of sign |
| 22 | #7 RefSieve: unknown-id play divides by the candidate count after a mistake (NaN with none) |
| 23 and 19 | #8 RefSieve: interpret_2p_locked_clue reads inferences the clue just wrote; Identity(suit, 6) |
| 26 | #9 H-Group: eval_action divides by the number of candidates after a mistake |
| 25 | #10 H-Group: xmeta returns a default beyond the stored vector but xmeta_mut grows it |
| 3 | #11 Endgame: winnable_if stores a drawn card with the wrong order |
| 5 | #12 Endgame: RemainingMap iteration order differs from Scala's Map |
| 6 | #13 Endgame: solver calls thinks_playables with assume = true |
| 36 | #14 Endgame solver often hits its time limit |
| 17, 18, 30, 32, 33 | #15 Game::elim / update_hypo_stacks are still most of the run time |
| 29 | #16 5+ players: lookahead is capped; quality unmeasured |
| 28 | #17 H-Group: giver and receiver read a re-clue differently (blind-play strikeout) |
| 37 | #18 analyze: endgame 'mistake!' comments about people can be spurious |
| 38 | #19 Library: hook for fetching exports; CI for the tokio-free build |
| (no item) | #20 RefSieve at 5 players averages only about 18.7, #21 Port the benchmark tool, #22 Clear the remaining clippy warnings

Upstream issues (section E) are issues #23-36 with the `upstream` label, titled `[upstream #N]`. New problems from real games go in the tracker
(there is a bug report template); add a FIX_LATER item only when there is background worth keeping.

Status: **open**, **fixed**, **resolved** (nothing to fix) or **kept** (a deliberate difference from Scala).

## A. Scala oddities still in the Rust (open)

Places where the Scala looks unintended and the Rust copied the behaviour so parity stayed measurable.

### Endgame solver

1. **fixed** `EndgameSolver::optimize` (`solver.rs`) only reduced `rem_prob` for arrangements rejected before search, so the pruning bound
   `winrate + rem_prob < best` was looser than intended. It is now reduced after every arrangement. The solver returns the same answers
   (all endgame tests pass); the measured speed-up on the tests was within noise (one test 19 s -> 15-18 s, another 7.5 s -> 8.5 s of CPU).
2. **fixed** `EndgameSolver::solve`, multi-arrangement loop: `rem_prob` started at `1 - winrate` instead of `1 - first.prob`.
3. **open** `winnable_if` (`winnable.rs`): the drawn `Card` is created with order `next_card_order + 1` but stored at
   `next_card_order`. Only the `order` field is wrong; probably harmless.
4. **fixed** `Game::analyze` (`basics/game/analyze.rs`) computed endgame winrates from the *final* game, not the replayed game `g`, so every
   endgame comment compared winrates from the wrong position. It now solves from `g`; the time per winrate is 500 ms, or
   `ANALYZE_ENDGAME_TIMEOUT_MS`. Caveat found while checking (item 37).
5. **open (low)** `RemainingMap` iteration order (ordinal) differs from Scala's unspecified `Map` order; only affects tie-breaks.
6. **open (low)** `thinks_playables` in the solver is called with `assume = true` (Scala default). Check whether `assume = false` is what
   endgame play-selection wants.

### Reactor

8. **open** `try_stable` (`reactor/interpret_clue.rs`): `copy(waiting = Option.when(game.waiting.isEmpty && ...))` *overwrites* an
   existing waiting connection with `None`. Probably meant `game.waiting.orElse(...)`.
9. **fixed** `interpret_discard`, Gentleman's Discard branch: the fold started from `game` instead of `g`, so the `check_missed` changes
   made to `g` were thrown away. It now continues from `g`.
10. **fixed** `interpret_discard`, "reset ctd after slot 1 discard": `prev.isTouched(0)` was called with *order 0*, not the discarded card, so the
    condition was nearly always met. It now asks whether the discarded slot-1 card was touched (`is_touched(order)`).
11. **open** `elim_dc_play` / `elim_play_play` (`reactor/interpret_reaction.rs`): `updateMeta(meta, ...)` is passed the original `meta`
    instead of the accumulated one, so only the last processed slot's `trash` flag survives.
12. **open** `calc_target_slot`: `indexOf` returns -1 when the reacting order isn't in the reacter's old hand, making the slot 0.
13. **open** `Reactor::must_clue`'s `unknownPlay` calls `IdentitySetOpt.get` on a possibly-empty `old_inferred`, which throws in Scala.
    The Rust treats "no old inference" as "different from the played identity".

### Referential Sieve

19. **fixed (partly)** `interpret_discard` (2p): the same `isTouched(0)` bug as item 10, fixed the same way. Still open: the log line does
    `prev.chop(..).get` (throws in Scala when there is no chop; the Rust prints the `Option`).
20. **resolved** RefSieve's Gentleman's Discard branch: `g` is still an untouched copy of `game` there, so it was never a problem.
21. **open** `eval_action` on a clue multiplies the result by `mult` regardless of its sign (Reactor only scales positive results).
22. **open** `eval_action` on an unknown-id play divides the sum by `uniqueInfs.length` even after a mistake (`-100 / n`), and is `NaN`
    when there are no unique inferences (`NaN` compares false, so such an action is never chosen over a real one).
23. **open** `interpret_2p_locked_clue` (colour branch) checks `thinksTrash(game, target)` on the *new* game's common, so it sees
    inferences the clue just wrote; and `delayed_plays` there can build `Identity(suit, 6)` for a finished suit, whose ordinal aliases the
    next suit's 1.

### H-Group

25. **open (low)** `HGroupGame::xmeta` returns a default for orders beyond the stored vector, while `xmeta_mut` grows it; the Scala
    `Vector.updated` would throw.
26. **open** `eval_action` for unknown-id plays/discards divides by the number of candidates even after a mistake (like item 22).
28. **open, design question** *Giver and receiver read a clue differently* (`investigations/hgroup5_seed9_3p.json`, `self_play seed=9
    convention=HGroup5`, 3p): at turn 24 Cathy re-clues Alice's already-clued 5s (no new cards touched, early game over). Alice's game
    marks her slot 1 `finessed` with inferred [g4,p4]; Cathy's own game leaves the card plain (she sees it is y1). Every later blind play
    (turns 25, 26, 29) follows and the game strikes out at 10. The Scala bot plays the same 30 actions (its `analyze` agrees with all of them), so
    this is inherited convention behaviour. Suspects: `interp_clue` for a clue that touches no new cards below level 6 (tempo clues), and
    the checks (`unacceptable_clue`, stalling) that should stop Cathy giving a clue her own simulation does not read as a play. Needs a
    convention-level decision, not a mechanical fix.

### Panics (policy)

14. **kept** Many `.get` / `.head` / `indexWhere` calls that throw in Scala are `expect`s in Rust (`ref_discard`'s target index,
    `delayed_plays`' `id.prev`, the `by` of a called-to-play card, ...). The client catches panics per action
    (see item 34 for what it does next).
15. **kept** `eval_dc`, `advance`: several `.get`s on `state.deck(..).id()` assume the card is visible.
24. **kept** Same policy for the H-Group port.

## B. Robustness (open)

34. **fixed** When `take_action` panicked, `BotClient::take_turn` used to log and return without acting, so the table waited on the bot.
    It now makes a safe fallback move (`fallback_action`: discard the oldest unclued card, or a rank/colour clue at 8 clue tokens), says
    "Sorry, I hit an internal error..." at the table, and appends an `"kind": "error"` line (turn, convention, seed, panic message) to
    `feedback.jsonl`. `analyze_history` shows these next to the matching game.

## C. Performance (history and what remains)

17. **resolved** `Game::clone` scaled with history. H-Group's `Ext` data, `State::action_list` and `Game::notes` are now `Arc`s copied on
    write; a clone went from 137 to 73 allocations (~55 us to ~5 us, `tests/clone_probe.rs`). Remaining clone cost: `State` (deck, discard
    stacks, names), the players' `thoughts` and `meta`.
18. **resolved (mostly)** `update_hypo_stacks` cloned the game per call: see 30, 32, 33. It is still ~75% of `Game::elim`, which is ~55% of
    self-play time (mostly the per-player hypothetical plays).
16. **resolved** `Convention::uses_refresh_after_play(&self, &Game)` is new (not in Scala): hypothetical plays only copy the game when it
    returns true. H-Group says so only when connections are waiting.
29. **kept** H-Group at 5+ players: the full lookahead (`advance`/`force_clue_value` over every clue of every other player, ~36 options per ply)
    does not finish its first decision in minutes, in Scala either. `advance` now stops after `MAX_LOOKAHEAD_5P = 1` plies (RefSieve too).
    2-4 players are unchanged. Quality at 5p vs. a deeper lookahead is unmeasured (RefSieve 5p averages ~18.7 over 6 games, H-Group 5p
    ~22.8). Candidates: prune the clue options instead of cutting depth.
30. **resolved** `hypo_play` copied the game twice per hypothetical play even with nothing waiting (102 s -> 52 s for 2 games).
31. **kept** `SELF_PLAY_SLOW_MS=<ms>` makes `self_play` print every decision that takes at least that long (to stderr).
32. **resolved** See 17 (102 s -> 27 s in total for 2 games of HGroup5 self-play).
33. **resolved** `update_hypo_stacks` computed `thinks_trash` every iteration only to filter for inverted suits; now skipped without inverted
    suits (22.3 s -> 9.8 s). `order_trash` is generally expensive; other hot callers of `thinks_trash` may benefit from caching.

36. **open** The endgame solver often runs into its 3 s limit in self-play (65 decisions of 2.9 s or more in 6 HGroup3 games on a busy
    machine), so endgames may be played from a partial search. `HANABI_ENDGAME_TIMEOUT_MS` (live bot) raises the limit
    (`endgame::solver::set_default_timeout`). The real fix is a faster search: most of its time is in `Game::simulate_action`, i.e. the
    clone/`elim`/`update_hypo_stacks` costs of section C.

37. **open** Endgame "mistake!" comments from `analyze` can be spurious for people: the solver works from the commenting player's
    point of view with the bot's *conventional* inferences, so a person who deviated from the bot's conventions can hold a hand the model
    thinks impossible and every action of theirs scores 0. Example: replay 2045738, turn 56 (MichielBaird discards b3, winrate 0 vs. 1
    for g4), yet that game finished 25/25 with that discard. Treat endgame comments about people with suspicion; for the bots' own moves
    they are meaningful. Possible fix: solve from the true hand (the analysis knows the whole deck) rather than the hidden-hand view.

38. **kept** The library does not depend on tokio or any network crate (`net` feature, see PORTING_NOTES.md). Not yet automated: nothing in CI
    stops a network dependency creeping back into the default-feature-free build (there is no CI yet). Also open: `/analyze` in chat and
    `GameData::fetch_id` just fail without `net`; an embedder might prefer a hook (a trait the app implements to fetch an export).

## D. Other resolved items

7. **resolved** Scala's `solve` left the log level at Off after the `onlyAction` early return; the Rust restores it.
27. **resolved** H-Group strength vs. Scala has been measured (HANDOFF.md, "Scala parity"): HGroup5 24.43 vs 24.33 average over 30 games each;
    HGroup3 24.47 vs 24.0; RefSieve 23.67 vs 24.37.
35. **kept** Live bots announce `/feedback` at the start of a game (but not again to the same people within 24 h, so "Play again" doesn't repeat it) and append JSON lines to `feedback.jsonl` (not in Scala).

## E. Upstream issues (will-hanabi-bot/scala-bot), checked 2026-10-28

Our port is exactly upstream's latest commit (`bcf85bb`, v1.2.8, 2026-10-21), so every **closed** upstream issue (the other 77 of 92) is already
fixed in what was ported; the ported tests carry many of the regressions. The **15 open** ones are unfixed upstream, therefore present here too.
None of the H-Group ones is below level 11, so none affects HGroup 1-10 play as reported.

| # | Where | What | In ours | Verdict |
|---|---|---|---|---|
| 91 | all | Run on another hanab.live server (`HANABI_HOSTNAME`); an old server omits `options` / `maxPlayers` from table messages, which Scala's JSON parser rejects | `HANABI_HOSTNAME` is supported; `Table` etc. use `#[serde(default)]`, so missing keys are fine | **Done** (not verified against such a server) |
| 92 | H11, orange | Gentleman's discard "through" an orange card isn't treated like a layered playable | yes | Variant + level 11; later |
| 89 | H11, orange | Orange 1 misdiscard read as a Gentleman's discard; also clues several 1s including orange ones | yes | Later |
| 88 | H11 | A prompt through a baton-discarded card isn't considered (4 clue read as self-finesse) | yes | Later |
| 87 | H11, orange | Screaming-discard chop move not recognised when the discarded card turns out orange | yes | Later |
| 86 | H11 | Illegal fill-in/tempo clue stall given when a 5 stall exists; asymmetric stall | yes | Same class as item 28 (common-knowledge desync) |
| 84 | H11 | Bot misses a stall clue while in DDA (`asymmetric! only [jabot2] think stall`) | yes | Same class as item 28 |
| 82 | H11, orange | Orange chop card not inferred as known trash (inferred o4) | yes | Later |
| 78 | H12 | Marks a card known (g3) although a finesse might be ambiguous | yes | Maintainer: by design |
| 64 | H12 | Clues r2 in the partner's hand without realising it has r2 itself | yes | Needs the replay; later |
| 61 | H12, black | Black save-clue exception for avoiding bad touch not implemented | yes | Maintainer: hesitant (breaks common-knowledge symmetry); a design discussion |
| 72 | Reactor, pinkish 5s | Stable reference-discard clues don't give the rank promise | yes | Variant-specific; later |
| 45 | Reactor | A clue meant as reactive is read as stable by one bot (desync in one edge case) | yes | Needs the replay |
| 35 | Reactor | ZCSP (zero-clue stall) should last while the previous player had 0 clues on their turn, but ends after one round (`reactor.rs` around line 425: reset when `prev` could clue, set when clues run out) | yes | Concrete spec; worth doing if Reactor is played |
| 33 | Reactor | A player whose card was called to play (g5, known since turn 52) should count as loaded; bot bombed an unplayable card | maybe (upstream changed empathy-after-clue in 0.10.10) | Needs the replay |

Takeaways: nothing here needs doing for the HGroup 1-3 bots now running. Of the open issues, #35 is the only one with an exact fix described, and #84, #86
and item 28 are the same problem (the giver and the receivers don't see the same thing) and are the more valuable thing to understand. Real games
from `analyze_history` and `/feedback` are a better source of bugs for the levels that are actually played.
