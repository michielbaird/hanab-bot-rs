//! A clue that its giver and its receiver read differently, in a real game: game 2066167 of hanab.live (`HGroup5`, 4 players,
//! 6 Suits; seats: michiel-bot-2, tolkand (a human), michiel-bot-3, michiel-bot-1).
//!
//! Turn 7: bot-1 clues 2 to bot-3, touching a new r2 (order 19, slot 1) and the chop (order 11, a t2). Turn 8: bot-2 clues 3
//! to tolkand's g3. Bot-2 can see that bot-3's 19 is r2, so it reads the clue as a finesse on tolkand's g2 (order 18). Bot-3
//! cannot see its own card: to it, the clued 2 could be the g2, so it reads a prompt and takes slot 1 for the g2. Turn 9:
//! tolkand clues red to bot-1's r3, which needs the r2. Bot-3 has already ruled slot 1 out for r2, so it prompts the other
//! clued 2, the chop, and plays it at turn 10: a teal 2, a bomb.

use hanab_bot::basics::{Game, PerformAction};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game, load_variant};
use std::collections::HashMap;

const EXPORT: &str = include_str!("fixtures/prompt_2066167.json");

/// The game as the player in `seat` sees it, before the action number `turn` (0-based).
fn game_before(turn: usize, seat: usize) -> Game {
    logger::set_level(LogLevel::Off);
    let mut data = GameData::parse(&serde_json::from_str(EXPORT).unwrap()).expect("the export");
    data.actions.truncate(turn);
    let variant = load_variant("6 Suits", &HashMap::new()).unwrap();
    build_game(ConventionKind::HGroup(5), &data, variant, seat, true).expect("a game")
}

const BOT_3: usize = 2;

/// Bot-3 plays the chop (order 11, a t2) at turn 10, as the game did.
#[test]
fn bot_3_plays_its_chop_as_the_prompted_r2() {
    let action = game_before(10, BOT_3).take_action();
    assert!(
        matches!(action, PerformAction::Play { target: 11 }),
        "{action:?}"
    );
}

/// It should not: slot 1 (order 19) is the card that was clued 2 and could be r2, and the clue that ruled it out for r2 was read
/// differently by its giver.
#[test]
#[ignore = "#119: the giver and the receiver read a clue that connects through a clued card differently"]
fn bot_3_does_not_play_its_chop_as_the_prompted_r2() {
    let action = game_before(10, BOT_3).take_action();
    assert!(
        !matches!(action, PerformAction::Play { target: 11 }),
        "bot-3 played its chop again: {action:?}"
    );
}
