# Oddities and follow-ups

The port is finished and now diverges from the Scala bot on purpose ("forking mode"): anything below can be fixed, one at a
time, re-running the tests (`cargo test -j2`) and a self-play sweep after each. Item numbers are stable (other docs refer to them).

Status: **open**, **fixed**, **resolved** (nothing to fix) or **kept** (a deliberate difference from Scala).

## A. Scala oddities still in the Rust (open)

Places where the Scala looks unintended and the Rust copied the behaviour so parity stayed measurable.

### Endgame solver

1. **open** `EndgameSolver::optimize` (`solver.rs`): `rem_prob` is only reduced for arrangements rejected before search, not after a
   winnable/unwinnable search result. The pruning bound `winrate + rem_prob < best` is therefore looser than intended, so the solver
   searches more than it needs to (and often hits its 3 s timeout live). Fix = subtract `prob` in every branch.
2. **open** `EndgameSolver::solve`, multi-arrangement loop: `rem_prob` starts at `1 - winrate` (the action's winrate from the first
   arrangement) instead of `1 - first_prob`. Same effect as 1.
3. **open** `winnable_if` (`winnable.rs`): the drawn `Card` is created with order `next_card_order + 1` but stored at
   `next_card_order`. Only the `order` field is wrong; probably harmless.
4. **open** `Game::analyze` (`basics/game/analyze.rs`): endgame winrates call `solver.solve(self, ..)` on the *final* game, not the replayed
   game `g`, so every endgame comment compares winrates from the wrong position. (Matters for `analyze_history`.)
5. **open (low)** `RemainingMap` iteration order (ordinal) differs from Scala's unspecified `Map` order; only affects tie-breaks.
6. **open (low)** `thinks_playables` in the solver is called with `assume = true` (Scala default). Check whether `assume = false` is what
   endgame play-selection wants.

### Reactor

8. **open** `try_stable` (`reactor/interpret_clue.rs`): `copy(waiting = Option.when(game.waiting.isEmpty && ...))` *overwrites* an
   existing waiting connection with `None`. Probably meant `game.waiting.orElse(...)`.
9. **open** `interpret_discard`, Gentleman's Discard branch: the fold starts from `game`, not `g`, so the `check_missed` (and `waiting`)
   changes made to `g` are thrown away.
10. **open** `interpret_discard`, "reset ctd after slot 1 discard": `prev.isTouched(0)` is called with *order 0*, not slot 1
    (`prev.state.hands(playerIndex)(0)`). False for almost every game, so the condition is nearly always met.
11. **open** `elim_dc_play` / `elim_play_play` (`reactor/interpret_reaction.rs`): `updateMeta(meta, ...)` is passed the original `meta`
    instead of the accumulated one, so only the last processed slot's `trash` flag survives.
12. **open** `calc_target_slot`: `indexOf` returns -1 when the reacting order isn't in the reacter's old hand, making the slot 0.
13. **open** `Reactor::must_clue`'s `unknownPlay` calls `IdentitySetOpt.get` on a possibly-empty `old_inferred`, which throws in Scala.
    The Rust treats "no old inference" as "different from the played identity".

### Referential Sieve

19. **open** `interpret_discard` (2p): `prev.isTouched(0)` is called with *order 0*, not slot 1 (as item 10), and the log line does
    `prev.chop(..).get` (throws in Scala when there is no chop; the Rust prints the `Option`).
20. **open** `interpret_discard`, Gentleman's Discard: starts from `game` again, like Reactor (item 9).
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

34. **open** When `take_action` panics, `BotClient::take_turn` only logs "panic while choosing an action!" and returns without acting, so the
    table waits on the bot until someone kicks it. It should make a safe fallback move (discard the oldest unclued card, or a clue at 8 clue
    tokens), tell the table, and record the failure (turn, convention, seed, panic message) where `/feedback` entries go.

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

## D. Other resolved items

7. **resolved** Scala's `solve` left the log level at Off after the `onlyAction` early return; the Rust restores it.
27. **resolved** H-Group strength vs. Scala has been measured (HANDOFF.md, "Scala parity"): HGroup5 24.43 vs 24.33 average over 30 games each;
    HGroup3 24.47 vs 24.0; RefSieve 23.67 vs 24.37.
35. **kept** Live bots announce `/feedback` at the start of each game and append JSON lines to `feedback.jsonl` (not in Scala).
