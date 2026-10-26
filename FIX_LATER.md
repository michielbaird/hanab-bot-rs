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
