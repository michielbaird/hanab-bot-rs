//! Playing a clued 1 that links one reading of a clue is not a mistake just because it rules out the other readings.

use std::collections::HashMap;

use hanab_bot::basics::PerformAction;
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game, load_variant};

/// Replay 2063608 (`HGroup4`, 4 players) before michiel-bot-2's 12th action: its clued 1 (order 20) is r1 or g1, and
/// the 3 clue on order 13 reads as r3 (blind-playing r1 and r2) or as b3 (prompting b2).
#[test]
fn playing_a_clued_1_that_rules_out_another_reading_is_not_a_mistake() {
    logger::set_level(LogLevel::Off);
    let path = format!(
        "{}/investigations/replay_2063608.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut data = GameData::from_file(&path).expect("the saved replay");
    data.actions.truncate(11);
    let variant = load_variant(&data.options.variant_name, &HashMap::new()).unwrap();

    let game = build_game(ConventionKind::HGroup(4), &data, variant, 3, true).unwrap();
    assert_eq!(game.take_action(), PerformAction::Play { target: 20 });
}
