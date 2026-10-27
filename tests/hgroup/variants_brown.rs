#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::action::ClueAction;
use hanab_bot::basics::action::{Action, DiscardAction};
use hanab_bot::basics::clue::{BaseClue, ClueKind};
use hanab_bot::basics::clue_result::{bad_touch_result, playables_result};
use hanab_bot::basics::connection::Connection;
use hanab_bot::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use hanab_bot::basics::player::Link;
use hanab_bot::basics::{CardStatus, Identity, PerformAction};
use hanab_bot::collections::FastBitSet;
use hanab_bot::endgame::EndgameSolver;
use hanab_bot::frac::Frac;
use hanab_bot::hgroup::state_eval::eval_action_impl;
use hanab_bot::hgroup::state_eval::get_result;
use hanab_bot::hgroup::{DcStatus, FStatus, HGroupGame, StallInterp};

use Player::*;

#[test]
fn understands_n2_5_save_with_brown() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Brown5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues brown to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["n1", "n2", "n5"]);
}

#[test]
fn understands_brown_save_with_dark_brown() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::DBrown5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues brown to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["n1", "n2", "n3", "n4", "n5"]);
}

#[test]
fn will_save_n5_with_brown() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "n5"],
        ],
        Setup {
            variant: TestVariant::Brown5,
            ..Setup::default()
        },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: 4
        }
    );
}

#[test]
fn plays_the_correct_card_after_a_delayed_play_clue() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "b1", "r2", "r3", "n5"],
            &["g4", "n1", "b3", "g4", "y3"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Brown5,
            play_stacks: Some(vec![0, 0, 0, 0, 3]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues brown to Alice (slots 2,4,5)");
    take_turn(&mut game, "Cathy clues brown to Bob");
    // Alice's slot 5 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][4]]
    );
}

#[test]
fn plays_the_correct_card_after_a_finesse() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["n5", "b1", "r2", "r3"],
            &["g3", "y3", "r1", "n1"],
            &["n3", "y1", "b3", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Brown5,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            discarded: &["n3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues brown to Alice (slots 2,3,4)");
    take_turn(&mut game, "Cathy clues brown to Bob");
    take_turn_draw(&mut game, "Donald plays n3", "y2");
    // Alice's slot 4 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}

#[test]
fn understands_a_brown_tempo_clue() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "b1", "r2", "r3", "n5"],
            &["g4", "n1", "b3", "g4", "y3"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Brown5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues brown to Alice (slots 3,4,5)");
    take_turn(&mut game, "Alice clues brown to Bob");
    take_turn(&mut game, "Bob clues brown to Alice (slots 3,4,5)");
    take_turn(&mut game, "Cathy clues blue to Bob");
    // Alice's slot 5 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][4]]
    );
    // Slot 2 should not be chop moved.
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn interprets_a_loaded_brown_clue() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Brown5,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues brown to Alice (slot 5)");
    // Not a save clue.
    has_infs(&game, None, Alice, 5, &["n1"]);
}

#[test]
fn doesn_t_interpret_a_loaded_brown_clue_when_only_giver_is_loaded() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Brown5,
            init: Box::new(|g| {
                pre_clue(g, Bob, 1, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues brown to Alice (slot 5)");
    // Potentially a save clue.
    has_infs(&game, None, Alice, 5, &["n1", "n2", "n5"]);
}
