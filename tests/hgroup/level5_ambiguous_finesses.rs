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
fn understands_an_ambiguous_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "g2", "r4", "y4"],
            &["r3", "y4", "b4", "b4"],
            &["g1", "p4", "p4", "r3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");
    let g = &game;
    has_status(g, Donald, 1, CardStatus::Finessed);
    take_turn_draw(&mut game, "Donald discards r3", "b1");
    has_infs(&game, None, Alice, 1, &["g1"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_an_ambiguous_finesse_with_a_self_component() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p4", "r4", "y4", "y4"],
            &["r1", "y5", "b4", "b4", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 3)");
    let g = &game;
    has_status(g, Cathy, 1, CardStatus::Finessed);
    take_turn_draw(&mut game, "Cathy discards g4", "r1");
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn recognizes_an_ambiguous_self_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "p2", "r2", "g4"],
            &["p1", "b3", "g2", "b4"],
            &["y2", "y4", "r1", "g3"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 2 to Bob"); // self-finesse on p1
    has_status(&game, Bob, 1, CardStatus::Finessed);
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)"); // Bob thinks it's on Cathy
    has_status(&game, Bob, 1, CardStatus::Finessed);
    take_turn(&mut game, "Cathy clues red to Donald"); // Cathy passes back
    has_status(&game, Bob, 1, CardStatus::Finessed);
}

#[test]
fn recognizes_an_ambiguous_self_finesse_when_a_direct_clue_is_impossible() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4"],
            &["y2", "g4", "g4", "b4"],
            &["g2", "p4", "g5", "p4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            discarded: &["y2"],
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Alice (slot 2)");
    let g = &game;
    has_status(g, Alice, 1, CardStatus::Finessed);
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_infs(&game, None, Alice, 2, &["r2"]);
}

#[test]
fn recognizes_an_ambiguous_reverse_finesse_where_the_finessed_card_is_discarded() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "r4", "r4", "p4", "y4"],
            &["r2", "b5", "p5", "y5", "r3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Cathy, 5, &["red"]);
                pre_clue(g, Cathy, 2, &["5"]);
                pre_clue(g, Cathy, 3, &["5"]);
                pre_clue(g, Cathy, 4, &["5"]);
            }),
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Cathy");
    take_turn_draw(&mut game, "Cathy discards r2", "p3");
    has_infs(&game, None, Alice, 1, &["r2"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn plays_into_an_ambiguous_self_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "b3", "g2", "b4"],
            &["y2", "y4", "r1", "g3"],
            &["b2", "g1", "y1", "r4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Alice (slots 2,3)"); // self-finesse on p1 (seems to be on Bob)
    has_status(&game, Bob, 1, CardStatus::Finessed);
    take_turn(&mut game, "Alice clues 1 to Donald");
    take_turn(&mut game, "Bob clues red to Cathy"); // Bob passes back
    // Alice now knows she is self-finessed.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["y1", "g1", "b1", "p1"]);
}

#[test]
fn doesn_t_expect_a_passback_if_no_other_ids_are_possible() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "p2", "r2", "g4"],
            &["p1", "b3", "g2", "b4"],
            &["y2", "y4", "r1", "g3"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)"); // Bob thinks it's on Cathy
    // Cathy should now know to play p1.
    has_status(&game, Bob, 1, CardStatus::None);
    has_status(&game, Cathy, 1, CardStatus::Finessed);
}

#[test]
fn passes_back_a_layered_ambiguous_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "g2", "g4", "r5"],
            &["g3", "y4", "p4", "y3"],
            &["r1", "r2", "b4", "p3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    take_turn_draw(&mut game, "Donald discards p3", "b3");
    // Alice should pass back.
    has_status(&game, Alice, 1, CardStatus::None);
    has_status(&game, Donald, 2, CardStatus::Finessed);
}

#[test]
fn reacts_to_an_ambiguous_finesse_passback() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "g4", "b4", "y4"],
            &["r3", "g2", "g4", "r5"],
            &["g3", "y4", "p4", "y3"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Cathy");
    take_turn(&mut game, "Alice clues 5 to Cathy"); // We think the finesse is on Bob
    take_turn_draw(&mut game, "Bob discards y4", "p3"); // Bob passes back
    // Alice should play now.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_status(&game, Bob, 1, CardStatus::None);
}

#[test]
fn allows_ambiguity_on_the_2nd_finessed_card() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "g4", "b4", "r5"],
            &["r2", "g4", "r3", "y4"],
            &["g3", "y4", "r4", "y3"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Cathy");
    take_turn(&mut game, "Alice clues red to Donald"); // We could be ambiguous for r1 or r2
    take_turn_draw(&mut game, "Bob plays r1", "p3");
    take_turn_draw(&mut game, "Cathy discards y4", "r1"); // Cathy demonstrates not on her
    // Alice knows she is ambiguous for r2.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["r2"]);
}
