//! The benchmark compares versions of the code by playing the same seeded games and comparing what was played, which only
//! works if self-play is deterministic. With the endgame solver off (its time limit makes it depend on the machine) it must be.

use hanab_bot::endgame::solver;
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::load_variant;
use hanab_bot::tools::self_play::{Rng, full_deck, simulate_game};
use std::collections::HashMap;

fn play(convention: ConventionKind, players: usize, seed: u64) -> Vec<String> {
    let variant = load_variant("No Variant", &HashMap::new()).unwrap();
    let mut deck = full_deck(&variant);
    Rng::new(seed).shuffle(&mut deck);
    let summary = simulate_game(&convention.build().unwrap(), &variant, players, &deck);
    assert!(summary.error.is_none(), "{:?}", summary.error);
    summary.actions.iter().map(|a| format!("{a:?}")).collect()
}

#[test]
fn self_play_is_the_same_every_time_without_the_solver() {
    logger::set_level(LogLevel::Off);
    solver::set_enabled(false);

    for convention in [
        ConventionKind::HGroup(2),
        ConventionKind::Reactor,
        ConventionKind::RefSieve,
    ] {
        let first = play(convention, 2, 314);
        let second = play(convention, 2, 314);
        assert!(
            first.len() > 15,
            "{convention}: a game should have more than {} moves",
            first.len()
        );
        assert_eq!(first, second, "{convention}");
    }

    // A different deck is a different game, so comparing the moves can tell games apart.
    assert_ne!(
        play(ConventionKind::HGroup(2), 2, 314),
        play(ConventionKind::HGroup(2), 2, 315)
    );
}
