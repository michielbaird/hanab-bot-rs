//! A cascade of bombs in a real game: game 2065891 of hanab.live (`HGroup5`, 4 players, No Variant). At turn 14 a human clues
//! yellow to michiel-bot-1, touching y4 and y3, with the y2 in his own hand. Each bot in turn then plays a clued card as the y2
//! that the clue promised, and bombs: bot-2 (turn 15, b2), bot-3 (turn 16, p2) and bot-1 (turn 17, y3).
//!
//! The first play follows from the clue. What this checks is the rest: once a play that the clue promised has bombed, the
//! bots after it should not go on trusting the same promise.

use hanab_bot::basics::{Game, PerformAction};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game, load_variant};
use std::collections::HashMap;

const EXPORT: &str = include_str!("fixtures/cascade_2065891.json");

/// The game as the player in seat `index` sees it, before the action number `turn` (0-based) is taken.
fn game_before(turn: usize, index: usize) -> Game {
    logger::set_level(LogLevel::Off);
    let mut data = GameData::parse(&serde_json::from_str(EXPORT).unwrap()).expect("the export");
    data.actions.truncate(turn);
    let variant = load_variant("No Variant", &HashMap::new()).unwrap();
    build_game(ConventionKind::HGroup(5), &data, variant, index, true).expect("a game")
}

/// The seats are michiel-bot-3, michiel-bot-1, herricku, michiel-bot-2.
const BOT_3: usize = 0;
const BOT_1: usize = 1;
const BOT_2: usize = 3;

fn plays(action: PerformAction, order: usize) -> bool {
    matches!(action, PerformAction::Play { target } if target == order)
}

/// What the game did (the bot's own move, before the clue was in doubt): bot-2 played its clued 2 (order 12, b2) as the y2.
#[test]
fn bot_2_plays_the_clued_2_as_the_promised_y2() {
    let action = game_before(15, BOT_2).take_action();
    assert!(plays(action, 12), "{action:?}");
}

/// After bot-2's play bombed, bot-3 should not play its own clued 2 (order 0, p2) as the y2 that the clue promised.
#[test]
#[ignore = "#117: the bots go on trusting the clue after a play it promised has bombed"]
fn bot_3_does_not_play_a_clued_2_after_the_promised_play_bombed() {
    let action = game_before(16, BOT_3).take_action();
    assert!(
        !plays(action, 0),
        "bot-3 played its clued 2 again: {action:?}"
    );
}

/// And bot-1 should not then play the y3 (order 7) it takes for the y2.
#[test]
#[ignore = "#117: the bots go on trusting the clue after a play it promised has bombed"]
fn bot_1_does_not_play_the_yellow_clued_card_after_two_bombs() {
    let action = game_before(17, BOT_1).take_action();
    assert!(!plays(action, 7), "bot-1 played the y3 again: {action:?}");
}
