#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn recognizes_a_prompt_via_ref_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "y4", "y4", "g4"],
            &["g1", "b4", "r2", "p4", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob"); // getting b1. r1 clued
    take_turn(&mut game, "Alice clues blue to Cathy"); // getting g1. b4 clued
    take_turn_draw(&mut game, "Bob plays b1", "p3");
    take_turn_draw(&mut game, "Cathy plays g1", "p3");
    take_turn(&mut game, "Alice clues purple to Cathy"); // getting r2
    // Bob's r1 should be prompted.
    has_infs(&game, None, Bob, 2, &["r1"]);
}

#[test]
fn recognizes_a_prompt_via_fill_in() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "y4", "y4", "g4"],
            &["g1", "r2", "b4", "p4", "g4"],
        ],
        Setup {
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
                pre_clue(g, Cathy, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy"); // getting r2
    // Bob's r1 should be prompted.
    has_infs(&game, None, Bob, 2, &["r1"]);
}

#[test]
fn doesn_t_give_wrong_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r5", "y4", "g4"],
            &["g1", "r2", "b4", "p4", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob"); // getting b1. r1 and r5 clued
    take_turn(&mut game, "Alice clues red to Cathy"); // getting g1. r2 clued
    take_turn_draw(&mut game, "Bob plays b1", "r1");
    take_turn_draw(&mut game, "Cathy plays g1", "p3");
    take_turn(&mut game, "Alice clues 2 to Cathy"); // getting r2
    // This clue is nonsensical.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn recognizes_a_double_prompt_via_fill_in() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r2", "r1", "y4", "g4"],
            &["y4", "r3", "b4", "p4", "g4"],
        ],
        Setup {
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
                pre_clue(g, Bob, 3, &["red"]);
                pre_clue(g, Cathy, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Cathy");
    // Bob's r1 and r2 should be prompted.
    has_infs(&game, None, Bob, 3, &["r1"]);
    has_infs(&game, None, Bob, 2, &["r2"]);
}

#[test]
fn doesn_t_give_double_prompts_without_filling_in() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r2", "r1", "y4", "g4"],
            &["y4", "r3", "b4", "p4", "g4"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
                pre_clue(g, Bob, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues blue to Cathy");
    // This clue is nonsensical.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn plays_into_a_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "r2", "b4", "p4", "g4"],
            &["y4", "y4", "g4", "r4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["red"]);
                pre_clue(g, Bob, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob"); // getting r2
    // We should be prompted in slot 2 for r1.
    has_infs(&game, None, Alice, 2, &["r1"]);
}

#[test]
fn doesn_t_play_into_a_satisfied_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "y4", "b4", "p4"],
            &["r2", "y4", "g4", "r4"],
            &["g5", "b4", "r3", "y3"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["red"]);
                pre_clue(g, Bob, 1, &["red"]);
                pre_clue(g, Cathy, 1, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Cathy"); // prompting Bob's r2
    // We shouldn't be prompted in slot 1 for r1.
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][0]]
            .inferred
            .len()
            > 1
    );
}
