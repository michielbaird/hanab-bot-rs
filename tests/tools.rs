//! End-to-end checks of the offline tooling: self-play, export, and replay.

use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game, load_variant};
use hanab_bot::tools::self_play::{Rng, full_deck, simulate_game, summary_json};

#[test]
fn a_self_play_game_exports_and_replays_to_the_same_score() {
    logger::set_level(LogLevel::Off);
    let variant = load_variant("No Variant", &Default::default()).unwrap();
    let conv = ConventionKind::Reactor.build().unwrap();

    let mut deck = full_deck(&variant);
    Rng::new(3).shuffle(&mut deck);

    let summary = simulate_game(&conv, &variant, 2, &deck);
    assert_eq!(summary.error, None);

    let json = summary_json(&variant, 2, &deck, &summary);
    let data = GameData::parse(&json).unwrap();
    assert_eq!(data.deck, deck);
    assert_eq!(data.actions, summary.actions);

    let game = build_game(ConventionKind::Reactor, &data, variant, 0, true).unwrap();
    assert_eq!(game.state.score(), summary.score);
}
