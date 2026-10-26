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
fn understands_a_clandestine_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g1", "r1", "b2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 3)");
    let g = &game;
    has_infs(g, None, Alice, 3, &["r2", "g2"]);
    take_turn_draw(&mut game, "Cathy plays g1", "b1");
    let g = &game;
    // Alice's slot 3 should still be [r2,g2] to allow for a clandestine finesse.
    has_infs(g, None, Alice, 3, &["r2", "g2"]);
    take_turn(&mut game, "Alice discards b1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4", "g5");
    take_turn_draw(&mut game, "Cathy plays r1", "r1");
    // Alice's slot 4 (used to be 3) should just be r2 now.
    has_infs(&game, None, Alice, 4, &["r2"]);
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn understands_a_fake_clandestine_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g1", "r1", "b2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 3)");
    take_turn_draw(&mut game, "Cathy plays g1", "b1");
    take_turn(&mut game, "Alice discards b1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4", "g5");
    take_turn(&mut game, "Cathy clues 5 to Bob");
    // Alice's slot 4 (used to be 3) should just be g2 now.
    has_infs(&game, None, Alice, 4, &["g2"]);
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn understands_a_symmetric_clandestine_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "r3"],
            &["r2", "y1", "b2", "y3", "p3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    // r3 reverse, y3 clandestine
    has_infs(&game, None, Bob, 5, &["r3", "y3"]);
}

#[test]
fn doesn_t_give_illegal_clandestine_self_finesses() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "r1", "g1", "y3", "p3"],
            &["r4", "r4", "g4", "r5", "b4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn gives_finesses_that_forces_another_to_prevent_a_clandestine_self_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "r1", "g1", "y3", "p3"],
            &["g1", "r4", "g4", "r5", "b4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
    has_status(&game, Cathy, 1, CardStatus::Finessed);
}

#[test]
fn plays_into_a_finesse_to_prevent_a_clandestine_self_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g2", "r1", "g1", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Cathy");
    // We must have g1 on finesse, otherwise Cathy will bomb g2 as r2.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["g1"]);
}

#[test]
fn recognizes_fake_clandestine_finesses() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "r4", "p1", "b1"],
            &["b4", "y2", "y5", "b3"],
            &["b1", "b2", "y1", "p2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 4 to Cathy"); // triple finesse, but could be y
    take_turn(&mut game, "Bob clues 2 to Cathy"); // reverse finesse
    let g = &game;
    // Donald's y1 should be finessed, not our slot 1.
    has_status(g, Donald, 3, CardStatus::Finessed);
    has_infs(g, None, Donald, 3, &["y1"]);
    has_status(g, Alice, 1, CardStatus::None);
    take_turn(&mut game, "Cathy clues purple to Bob");
    take_turn_draw(&mut game, "Donald plays b1", "r5");
    // Donald's y1 should still be finessed.
    has_status(&game, Donald, 3, CardStatus::Finessed);
    has_infs(&game, None, Donald, 3, &["y1"]);
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn waits_for_a_clandestine_finesse_to_resolve() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p2", "g3", "y3", "g5"],
            &["r1", "p1", "y2", "b4"],
            &["r4", "y5", "b2", "r3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 0, 0, 2, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Alice (slots 2,3)");
    has_infs(&game, None, Alice, 2, &["r3", "b3", "p3"]);
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 5 to Donald");
    take_turn_draw(&mut game, "Cathy plays r1", "g4");
    take_turn_draw(&mut game, "Donald discards r3", "y1");
    assert!(
        game.me()
            .thinks_playables(&game, game.state.our_player_index, false, true)
            .is_empty()
    );
    take_turn(&mut game, "Alice clues yellow to Donald");
    take_turn_draw(&mut game, "Bob discards y3", "b1");
    let mut purple_f = game.clone();
    take_turn_draw(&mut purple_f, "Cathy plays p1", "p4");
    has_infs(&purple_f, None, Alice, 2, &["p3"]);
    assert!(
        purple_f.common.thoughts[purple_f.state.hands[Alice.ordinal()][0]]
            .inferred
            .len()
            > 1
    );
    has_status(&purple_f, Alice, 1, CardStatus::None);
    let mut red_f = game.clone();
    take_turn_draw(&mut red_f, "Cathy discards b4", "p4");
    has_infs(&red_f, None, Alice, 2, &["r3"]);
    has_infs(&red_f, None, Alice, 1, &["r2"]);
    has_status(&red_f, Alice, 1, CardStatus::Finessed);
}

#[test]
fn doesn_t_wait_for_a_potential_finesse_when_known_playable() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "r3", "y3", "g3", "b3"],
            &["r4", "y4", "b4", "g4", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 0, 2, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 4)");
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");
    // Our 2 is definitely playable, even if Bob is finessing y2.
    assert_eq!(
        game.me()
            .thinks_playables(&game, game.state.our_player_index, false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}

#[test]
fn understands_a_surreptitious_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p5"],
            &["r1", "b1", "b2", "b4", "p4"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 1)");
    has_infs(&game, None, Alice, 1, &["r3", "y3", "g3", "b3", "p3"]);
    take_turn_draw(&mut game, "Cathy plays r1", "r4");
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn_draw(&mut game, "Bob discards b4", "p4");
    take_turn_draw(&mut game, "Cathy plays b1", "g4");
    take_turn(&mut game, "Alice plays p1 (slot 2)");
    // Alice's 3 is definitely blue.
    has_infs(&game, None, Alice, 2, &["b3"]);
    // Alice's slot 3 is definitely not finessed, and Alice has no playables
    has_status(&game, Alice, 3, CardStatus::None);
    assert!(
        game.me()
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}
