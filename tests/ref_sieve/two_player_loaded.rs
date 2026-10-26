#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn interprets_a_loaded_rank_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y5", "y4", "b2"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 1, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 4 to Bob");
    has_status(&game, Bob, 5, CardStatus::DragToPlay);
}

#[test]
fn receives_a_loaded_rank_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y5", "y4", "b2"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2)");
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    take_turn(&mut game, "Bob clues 4 to Alice (slots 1,4)");
    has_status(&game, Alice, 5, CardStatus::DragToPlay);
    take_turn(&mut game, "Alice plays b1 (slot 2)");
    has_status(&game, Alice, 5, CardStatus::DragToPlay);
    has_infs(&game, None, Alice, 5, &["r2", "b2"]);
}

#[test]
fn interprets_a_loaded_colour_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "b4", "y5", "y4", "b1"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    has_infs(&game, None, Bob, 1, &["r1"]);
}

#[test]
fn receives_a_loaded_colour_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y5", "y4", "b2"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2)");
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    take_turn(&mut game, "Bob clues purple to Alice (slot 1)");
    has_infs(&game, None, Alice, 1, &["p1"]);
}

#[test]
fn discards_ptd_after_an_unknown_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "b4", "y5", "y4", "b3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob plays p1", "b4");
    // Even though Alice is loaded, she has PTD because Bob couldn't know he was playing p1.
    has_status(&game, Alice, 1, CardStatus::PermissionToDiscard);
    take_turn(&mut game, "Alice plays y2 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4 (slot 1)", "g1");
    // Alice should discard slot 2 from PTD.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn doesn_t_write_ptd_when_partner_plays_a_known_connector() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "b4", "y5", "y4", "b3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays p1", "b4");
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn loads_on_ptd() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "b4", "y5", "y4", "b3"],
        ],
        Setup {
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob plays p1", "b1");
    has_status(&game, Alice, 1, CardStatus::PermissionToDiscard);
    take_turn(&mut game, "Alice clues yellow to Bob");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 4)");
    // This is a LPC on slot 5.
    has_status(&game, Alice, 5, CardStatus::DragToPlay);
}

#[test]
fn revokes_ptd_after_a_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p5", "r4", "r5", "r3", "b3"],
        ],
        Setup {
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn_draw(&mut game, "Bob discards r4", "b1");
    has_status(&game, Alice, 1, CardStatus::PermissionToDiscard);
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn(&mut game, "Bob clues 4 to Alice (slot 1)");
    // Alice is called to discard slot 2 (ptd revoked).
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}
