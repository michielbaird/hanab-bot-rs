#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::action::ClueAction;
use scala_bot::basics::action::{Action, DiscardAction};
use scala_bot::basics::clue::{BaseClue, ClueKind};
use scala_bot::basics::clue_result::{bad_touch_result, playables_result};
use scala_bot::basics::connection::Connection;
use scala_bot::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use scala_bot::basics::player::Link;
use scala_bot::basics::{CardStatus, Identity, PerformAction};
use scala_bot::collections::FastBitSet;
use scala_bot::endgame::EndgameSolver;
use scala_bot::frac::Frac;
use scala_bot::hgroup::state_eval::eval_action_impl;
use scala_bot::hgroup::state_eval::get_result;
use scala_bot::hgroup::{DcStatus, FStatus, HGroupGame, StallInterp};

use Player::*;

#[test]
fn bluffs_on_top_of_differently_coloured_plays() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r2", "y1", "y1", "r4"],
            &["p5", "b2", "r3", "y5", "y4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    take_turn(&mut game, "Alice clues blue to Cathy");
    has_status(&game, Bob, 3, CardStatus::Bluffed);
}

#[test]
fn bluffs_on_top_of_differently_ranked_plays() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "p3", "y1", "b4", "r4"],
            &["g5", "g3", "y4", "y4", "r4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 0, 1, 0, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues 3 to Cathy");
    has_status(&game, Bob, 2, CardStatus::Bluffed);
}

#[test]
fn doesn_t_consider_invalid_bluff_targets() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "y2", "r2", "y4"],
            &["p3", "p1", "y1", "r4"],
            &["g5", "y1", "p4", "b5"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 1]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Donald");
    take_turn(&mut game, "Donald clues green to Alice (slots 3,4)");
    take_turn(&mut game, "Alice clues 3 to Cathy");
    take_turn_draw(&mut game, "Bob plays b1", "b4");
    // Cathy's card must be exactly p3 as a bluff.
    has_infs(&game, None, Cathy, 1, &["p3"]);
}

#[test]
fn doesn_t_confuse_a_bluff_for_a_layered_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "b1", "y4", "y3"],
            &["g4", "r5", "b2", "p4"],
            &["r1", "r1", "r3", "y1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Cathy");
    take_turn_draw(&mut game, "Bob plays g1", "b4");
    has_status(&game, Bob, 2, CardStatus::None);
    has_infs(&game, None, Cathy, 3, &["b2"]);
}

#[test]
fn disambiguates_a_bluff_finesse_when_demonstrated_1() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p2", "b4", "y1", "p4", "g4"],
            &["r3", "p3", "g1", "y2", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 1)");
    take_turn(&mut game, "Alice plays p1 (slot 2)");
    let mut finesse_game = game.clone();
    take_turn_draw(&mut finesse_game, "Bob plays p2", "r5");
    // Slot 2 (was slot 1) is known to be p3 as a double finesse.
    has_infs(&finesse_game, None, Alice, 2, &["p3"]);
    let mut bluff_game = game.clone();
    take_turn(&mut bluff_game, "Bob clues green to Cathy");
    // Slot 2 (was slot 1) is known to be [r3,y3] as a bluff.
    has_infs(&bluff_game, None, Alice, 2, &["r3", "y3"]);
}

#[test]
fn disambiguates_a_bluff_finesse_when_demonstrated_2() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "g4", "b4", "p4", "r4"],
            &["b1", "r4", "y3", "g3", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 2, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["1"]);
                pre_clue(g, Cathy, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)"); // r3, y3
    take_turn(&mut game, "Alice plays p1 (slot 1)"); // slot 1 now has no neg info (p1 isn't important)
    take_turn(&mut game, "Bob clues 4 to Cathy"); // could be r3 (Alice, playable) or b1 (Cathy, self-bluff)
    take_turn_draw(&mut game, "Cathy plays b1", "p3");
    let g = &game;
    // Alice is not finessed.
    has_status(g, Alice, 1, CardStatus::None);
    take_turn(&mut game, "Alice plays y3 (slot 2)");
    // Alice is still not finessed.
    has_status(&game, Alice, 2, CardStatus::None);
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}

#[test]
fn prompts_when_considering_whether_a_bluff_is_valid() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "g1", "p2", "y3"],
            &["r1", "b1", "r3", "b4"],
            &["b3", "p3", "r3", "r2"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Alice (slots 1,3)");
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    take_turn(&mut game, "Bob clues 4 to Alice (slot 1)"); // could be b2 prompt
    take_turn_draw(&mut game, "Cathy plays r1", "g2");
    has_infs(&game, None, Alice, 3, &["b3"]);
}

#[test]
fn interprets_a_bluff_when_a_hidden_finesse_is_impossible() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "g3", "b3", "m2"],
            &["r2", "y4", "g4", "y3"],
            &["g2", "b4", "m4", "y1"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Rainbow5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 2)"); // r1,m1
    take_turn(&mut game, "Cathy clues red to Bob"); // m2 (promising m1 in Alice's hand)
    take_turn(&mut game, "Donald clues red to Cathy"); // bluffing Alice (can't be hidden f)
    has_status(&game, Alice, 1, CardStatus::Bluffed);
}
