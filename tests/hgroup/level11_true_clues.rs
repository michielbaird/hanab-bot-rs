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
fn understands_a_fake_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "g3", "r3", "g5"],
            &["p1", "r4", "b5", "b2"],
            &["r2", "b2", "g1", "y3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Donald"); // b1 finesse on us
    take_turn_draw(&mut game, "Cathy discards b2", "y5");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b1"]);
    has_status(&game, Cathy, 2, CardStatus::None);
}

#[test]
fn removes_a_fake_bluff_waiting_connection() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "r4", "r4", "y4"],
            &["r2", "g4", "g4", "b4"],
            &["r3", "p4", "p4", "r1"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Donald, 4, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy");
    take_turn(&mut game, "Bob clues red to Donald");
    // Bob is definitely not bluffed anymore.
    let bob_first = game.state.hands[Bob.ordinal()][0];
    assert!(!game.waiting().iter().any(|wc| {
        wc.connections
            .iter()
            .any(|conn| conn.reacting() == Bob.ordinal() && conn.order() == bob_first)
    }));
}

#[test]
fn understands_a_direct_play_if_the_bluff_isn_t_played_into() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "y5", "b1", "g5", "p2"],
            &["b3", "r1", "b5", "b2", "y4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 2)");
    take_turn_draw(&mut game, "Cathy discards y4", "y1");
    has_infs(&game, None, Alice, 2, &["r3"]);
}

#[test]
fn understands_giving_a_direct_play_through_a_bluff_opportunity() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "r1", "b1", "g5", "p2"],
            &["p1", "r3", "b5", "b2", "y4"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Cathy");
    take_turn_draw(&mut game, "Bob discards p2", "y5");
    has_infs(&game, None, Cathy, 2, &["r3"]);
}

#[test]
fn understands_a_self_finesse_that_s_too_far_away_to_be_a_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g5", "y2", "b3", "y5"],
            &["p4", "r3", "g1", "y4"],
            &["p2", "b5", "b1", "y1"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 4 to Alice (slot 2)");
    // No 4 is a valid bluff target, so this must be a finesse.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["r1", "y1", "g1", "b1", "p1"]);
}

#[test]
fn recognizes_a_finesse_when_focus_is_not_a_valid_bluff_target() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "y3", "g1", "y1"],
            &["r2", "b1", "p5", "p1"],
            &["p2", "r1", "b2", "y1"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Donald");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["p1"]);
}

#[test]
fn assumes_a_reverse_finesse_over_a_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "r1", "g5", "g2"],
            &["b3", "w1", "w5", "w2"],
            &["r1", "r4", "b2", "y4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            variant: TestVariant::White5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::None);
    has_status(&game, Cathy, 1, CardStatus::Finessed);
    has_infs(&game, None, Cathy, 1, &["b3"]);
}

#[test]
fn assumes_a_finesse_over_a_self_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g4", "g2", "p1", "b2"],
            &["b4", "g4", "y4", "r4"],
            &["y3", "r1", "g3", "p3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 1, 0, 0, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Donald");
    take_turn(&mut game, "Donald clues 3 to Alice (slot 1)");
    // Since Alice has to respect r3, she can't be bluffed.
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn understands_a_double_finesse_when_the_target_is_too_far_away() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g4", "p3", "r3", "b4"],
            &["y1", "y2", "p2", "p4"],
            &["b1", "y5", "g2", "r4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![3, 4, 1, 1, 3]),
            discarded: &["r1", "y3", "g3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b2"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Alice, 2, &["b3"]);
}

#[test]
fn receives_a_fake_bluff_into_double_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5", "p2"],
            &["p1", "r4", "b5", "b2", "y4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 2)");
    take_turn_draw(&mut game, "Cathy discards y4", "y5");
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_a_double_reverse_finesse_when_the_target_is_too_far_away() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g3", "p4", "r2", "y1"],
            &["g4", "p1", "b4", "p3"],
            &["b4", "y2", "g5", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Donald");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["y1"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Alice, 2, &["y2"]);
}

#[test]
fn understands_a_layered_finesse_when_the_target_is_too_far_away() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y4", "p4", "r2", "y1"],
            &["g5", "b1", "b4", "y3"],
            &["g4", "p1", "g3", "p3"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Cathy");
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Alice, 2, &["y1"]);
}

#[test]
fn understands_a_clandestine_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y4", "y5", "y1", "y1"],
            &["g3", "g3", "b2", "b1"],
            &["g1", "r1", "r3", "y2"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 4)");
    take_turn_draw(&mut game, "Donald plays g1", "g5");
    // No 3 is a valid bluff target.
    has_infs(&game, None, Alice, 4, &["r3", "g3"]);
    has_status(&game, Donald, 2, CardStatus::Finessed);
}

#[test]
fn rank_connects_on_a_self_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "p1", "y1", "p4"],
            &["r3", "p3", "p1", "y2"],
            &["b3", "y2", "g4", "r5"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Alice (slot 3)");
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    has_infs(&game, None, Alice, 3, &["g2"]);
}

#[test]
fn doesn_t_allow_a_second_round_when_a_bluff_isn_t_played_into() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g3", "p4", "p3"],
            &["r5", "y5", "g4", "r1"],
            &["r3", "g1", "p2", "r3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            init: Box::new(|g| {
                fully_known(g, Donald, 3, "p2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues purple to Bob");
    take_turn_draw(&mut game, "Donald discards r3 (slot 4)", "r4");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["p1"]);
}

#[test]
fn doesn_t_play_into_a_bluff_when_a_self_finesse_prompt_is_possible() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r3", "y4", "g4", "b4"],
            &["p4", "p4", "y4", "g4", "b4"],
        ],
        Setup {
            starting: Cathy,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    // Alice should wait for Bob to demonstrate.
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn assumes_a_direct_play_even_when_an_ambiguous_layer_with_self_component_is_possible() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "g4", "b4"],
            &["r1", "g1", "y4", "g4", "b4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 1)");
    take_turn_draw(&mut game, "Cathy discards b4", "b5");
    // Alice should no longer assume it can be red or green.
    has_infs(&game, None, Alice, 1, &["p3"]);
}
