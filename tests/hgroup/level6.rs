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
fn understands_a_tccm() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r2", "g4", "r5", "b4"],
            &["g1", "b3", "r2", "y3", "p3"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "y5");
    take_turn(&mut game, "Cathy clues 2 to Bob");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn understands_a_tccm_on_self() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "y5", "g4", "r5", "b4"],
            &["g1", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 1,2)");
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    take_turn(&mut game, "Bob clues 2 to Alice (slot 2)");
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
}

#[test]
fn recognizes_a_tempo_clue_stall() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g2", "r4", "y3"],
            &["b5", "y5", "g5", "r5"],
            &["b4", "p3", "g1", "g1"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 5 to Cathy");
    take_turn(&mut game, "Alice clues green to Bob");
    take_turn_draw(&mut game, "Bob plays g1 (slot 1)", "y2");
    take_turn(&mut game, "Cathy clues 2 to Bob");
    has_status(&game, Bob, 4, CardStatus::None);
}

#[test]
fn doesn_t_tccm_if_getting_a_chop_moved_card() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g2", "r1", "y2"],
            &["b5", "y5", "g5", "r5"],
            &["b4", "p3", "g1", "g4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 1, 1, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 1 to Bob");
    take_turn(&mut game, "Alice clues yellow to Bob");
    has_status(&game, Bob, 2, CardStatus::None);
}

#[test]
fn doesn_t_tccm_if_getting_a_playable_in_other_hand() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "p2", "r1", "y1"],
            &["b2", "y5", "g3", "y3"],
            &["b4", "p4", "g1", "g1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 2, 2, 2, 0]),
            discarded: &["y3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues purple to Bob");
    take_turn(&mut game, "Donald clues 3 to Cathy");
    take_turn(&mut game, "Alice clues 2 to Bob"); // p2 play unlocks Cathy's touched g3
    has_status(&game, Bob, 4, CardStatus::None);
}

#[test]
fn doesn_t_tccm_if_the_card_was_already_playing_symmetrically() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b2", "r3", "r1", "y1"],
            &["p1", "y5", "g3", "y3"],
            &["b4", "p4", "g1", "g1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues purple to Cathy");
    take_turn(&mut game, "Bob clues purple to Cathy");
    has_status(&game, Cathy, 4, CardStatus::None);
}

#[test]
fn tccms_if_a_card_was_already_playing_but_unknown() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "r4", "r4", "y4"],
            &["g4", "g4", "b4", "b4", "y2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Cathy");
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues yellow to Cathy");
    has_status(&game, Cathy, 4, CardStatus::ChopMoved);
}

#[test]
fn prefers_interpreting_a_tccm_over_tcm() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "g4", "r5", "r4"],
            &["g3", "y4", "y1", "y5", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["5"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 2,5)");
    // Slot 4 should be chop moved, but not slot 3.
    has_status(&game, Alice, 4, CardStatus::ChopMoved);
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn interprets_a_simple_focus_shift() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Alice (slots 2,3)");
    take_turn(&mut game, "Cathy clues blue to Alice (slots 2,3)");
    // Note that this also causes a tempo clue chop move.
    has_infs(&game, None, Alice, 2, &["b1"]);
    has_infs(&game, None, Alice, 3, &["b2"]);
}

#[test]
fn interprets_a_focus_shift_when_touching_a_finessed_card() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4"],
            &["r4", "y4", "g4", "b4"],
            &["r2", "p4", "p4", "y3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Donald");
    let g = &game;
    has_infs(g, None, Alice, 1, &["r1"]);
    take_turn(&mut game, "Cathy clues red to Alice (slots 1,3)");
    has_infs(&game, None, Alice, 3, &["r3"]);
}
