# Oddities to fix after the port is done

Places where the Scala looks unintended but the Rust keeps the behaviour, so parity checks stay meaningful.
Fix these only once the port is complete and green, one at a time, re-running self-play/tests after each.
Add new entries as they are found while porting (file, what, why it looks wrong, risk).

## Endgame (M3)

1. `EndgameSolver::optimize` (`solver.rs`): `rem_prob` is only reduced for arrangements rejected before search,
   not after a winnable/unwinnable search result. The pruning bound `winrate + rem_prob < best` is therefore
   looser than intended. Fix = subtract `prob` in every branch. Risk: prunes more, may change tie-breaking.
2. `EndgameSolver::solve`, multi-arrangement loop: `rem_prob` starts at `1 - winrate` (the action's winrate from
   the first arrangement) instead of `1 - first_prob`. Same effect as 1: bound is too loose.
3. `winnable_if` (`winnable.rs`): the drawn `Card` is created with order `next_card_order + 1` but stored at
   `next_card_order`. Only the `order` field is wrong; probably harmless.
4. `Game::analyze` (`basics/game/analyze.rs`): endgame winrates call `solver.solve(self, ..)` on the *final*
   game, not the replayed game `g`, so every endgame comment compares winrates from the wrong position.
5. `RemainingMap` iteration order (ordinal) differs from Scala's unspecified `Map` order. Not a bug, but tests
   that depend on a specific tie-break among equal actions could differ from the Scala bot.
6. `thinks_playables` in the solver is called with `assume = true` (Scala default). Check whether
   `assume = false` is what endgame play-selection actually wants.
7. Scala's `solve` left the log level at Off after the `onlyAction` early return. Rust restores it (fixed
   by design, listed for completeness).

## Reactor (M4)

8. `try_stable` (`reactor/interpret_clue.rs`): `copy(waiting = Option.when(game.waiting.isEmpty && ...))` *overwrites*
   an existing waiting connection with `None` instead of leaving it alone. Probably meant `game.waiting.orElse(...)`.
9. `interpret_discard`, Gentleman's Discard branch: the fold starts from `game`, not `g`, so the `check_missed`
   (and `waiting`) changes made to `g` are thrown away.
10. `interpret_discard`, "reset ctd after slot 1 discard": `prev.isTouched(0)` is called with *order 0*, not slot 1
    (`prev.state.hands(playerIndex)(0)`). It is false for almost every game, so the condition is nearly always met.
11. `elim_dc_play` / `elim_play_play` (`reactor/interpret_reaction.rs`): `updateMeta(meta, ...)` is passed the original
    `meta` instead of the accumulated one, so only the last processed slot's `trash` flag survives (earlier ones are
    dropped). The Rust resets to a snapshot to match.
12. `calc_target_slot`: `indexOf` returns -1 when the reacting order isn't in the reacter's old hand, making the slot 0.
13. `Reactor::must_clue`'s `unknownPlay` calls `IdentitySetOpt.get` on a possibly-empty `old_inferred`, which throws in
    Scala. The Rust treats "no old inference" as "different from the played identity".
14. Many `.get` / `.head` / `indexWhere` calls that throw in Scala are `expect`s in Rust (e.g. `ref_discard`'s target index,
    `delayed_plays`' `id.prev`, the `by` of a called-to-play card). They panic on a broken invariant. M2 should decide
    whether the bot catches panics per action so one bad interpretation doesn't end the game.
15. `eval_dc`, `advance`: several `.get`s on `state.deck(..).id()` assume the card is visible.
16. `Convention::uses_refresh_after_play` is new (not in Scala): hypothetical plays only copy the game for conventions
    that override `refresh_after_play`. H-Group must return `true`.

## Performance (see HANDOFF.md)

17. `Game::clone` is cheap at the start of a game (~3µs) but scales with history (`State::action_list`,
    `last_actions`, `move_history`, `notes`, ... are deep-copied). Scala shared these structurally.
    Candidates: `Arc`-wrap the rarely-mutated history, or `im`-style persistent vectors.
18. `Player::update_hypo_stacks` clones the whole `Game` per call and `Game::elim` calls it for the common perspective
    and every player. ~60% of solver time in the Reactor endgame tests.

## Referential Sieve (M5)

19. `interpret_discard` (2p): `prev.isTouched(0)` is called with *order 0*, not slot 1, and the log line does `prev.chop(..).get`
    (throws in Scala when there is no chop; the Rust prints the `Option`).
20. `interpret_discard`, Gentleman's Discard: starts from `game` again, like Reactor (item 9).
21. `eval_action` on a clue multiplies the result by `mult` regardless of its sign (Reactor only scales positive results).
22. `eval_action` on an unknown-id play divides the sum by `uniqueInfs.length` even after a mistake (`-100 / n`), and is `NaN` when
    there are no unique inferences. Kept (`NaN` compares false, so such an action is never chosen over a real one).
23. `interpret_2p_locked_clue` (colour branch) checks `thinksTrash(game, target)` on the *new* game's common, so it sees inferences the clue
    just wrote; and `delayed_plays` there can build `Identity(suit, 6)` for a finished suit, whose ordinal aliases the next suit's 1.

## H-Group (M6)

24. Many Scala `.get` / `.head` calls that throw on a broken invariant are `expect`s in the Rust H-Group port (same policy as item 14).
25. `HGroupGame::xmeta` returns a default for orders beyond the stored vector, while `xmeta_mut` grows it; the Scala `Vector.updated` would throw.
26. `eval_action` for unknown-id plays/discards divides by the number of candidates even after a mistake (like item 22).
27. H-Group self-play strength has not been compared with the Scala bot (deferred, see HANDOFF.md item 0).

## Open investigation: HGroup5 seed 9 (3p) strikeout, `investigations/hgroup5_seed9_3p.json`

28. `self_play games=1 seed=9 convention=HGroup5` strikes out at score 10 (turns 25, 26, 29 are blind plays of unclued new cards).
    Turn 24: Cathy re-clues Alice's already-clued 5s ("5 to Alice", no new cards touched, 1-2 clues left, early game over).
    Alice's game marks her slot 1 (order 26) `finessed` with inferred [g4,p4] (reasoning turn 24); Cathy's own game leaves the same card
    plain `none` (she sees it is y1). Giver and receiver interpret the clue differently, and every later blind play follows from
    that. Same pattern at t27 (Alice sees a finesse for Bob's slot 1 too). `analyze` is silent because each move is consistent
    with the mover's own view. Unknown whether the Scala bot does the same: needs the Scala parity build (HANDOFF item 0).
    Suspects: `interp_clue` for a clue that touches no new cards below level 6 (tempo clues), and the stalling/`unacceptable_clue`
    checks that should stop Cathy giving a clue her own simulation does not read as a play.

    **Update:** built the Scala bot (scala-cli, JDK 25) and ran `scala_bot.analyze file=investigations/hgroup5_seed9_3p.json convention=HGroup5`:
    it reports no differing moves, so the Scala bot plays all 30 actions of this game, including the t24 clue and the three
    bombs. The behaviour is inherited from the Scala conventions, not a port divergence. (Seeds do not reproduce across the two
    implementations: the Rust PRNG is not Java's, so `selfPlay seed=9` in Scala is a different game and scored 24.)
