#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::interp::{ClueInterp, Interp};
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn recognizes_a_finesse_via_ref_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r4", "y4", "y4", "g4"],
            &["b2", "b4", "r2", "p4", "g4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Cathy");
    // Bob's b1 should be finessed.
    has_infs(&game, None, Bob, 1, &["b1"]);
    has_status(&game, Bob, 1, CardStatus::Finessed);
}

#[test]
fn recognizes_a_finesse_via_fill_in() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "b4", "y4", "g4"],
            &["b2", "r4", "y4", "p4", "g4"],
        ],
        Setup {
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 1, &["blue"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy");
    // Bob's b1 should be finessed.
    has_infs(&game, None, Bob, 1, &["b1"]);
    has_status(&game, Bob, 1, CardStatus::Finessed);
}

#[test]
fn plays_into_an_unknown_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "r4", "g4", "p4", "g4"],
            &["y4", "y4", "b4", "r4", "p4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    // We should be finessed in slot 1 for r1.
    has_infs(&game, None, Alice, 1, &["r1"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn doesn_t_play_into_a_satisfied_finesse() {
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
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues yellow to Cathy"); // finessing Bob's r1
    // We shouldn't be finessed in slot 1 for r1.
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn writes_the_correct_notes_on_potential_finesses() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "r4", "g4", "p4", "g4"],
            &["r1", "y4", "b4", "r4", "p4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 2)");
    // Alice's slot 1 could be any non-green 1, or r2.
    has_infs(&game, None, Alice, 1, &["r1", "r2", "y1", "b1", "p1"]);
    take_turn_draw(&mut game, "Cathy discards r1", "g5");
    // Alice's slot 1 can be any non-green 1.
    has_infs(&game, None, Alice, 1, &["r1", "y1", "b1", "p1"]);
}

#[test]
fn recognizes_a_finesse_via_fill_in_2() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "r4", "p4", "g4"],
            &["r2", "y4", "b4", "g4", "p4"],
        ],
        Setup {
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 1, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy");
    // Bob's slot 1 should be finessed.
    has_infs(&game, None, Bob, 1, &["r1"]);
    has_status(&game, Bob, 1, CardStatus::Finessed);
}

#[test]
fn plays_into_a_finesse_via_fill_in() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "r4", "r4", "p4", "g4"],
            &["y4", "y4", "b4", "g4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Bob, 1, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");
    // Alice's slot 1 should be finessed.
    has_infs(&game, None, Alice, 1, &["r1"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_a_prompt_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "y4", "b4", "p4"],
            &["r2", "y4", "g4", "r4"],
            &["g5", "b4", "r3", "y3"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Bob, 1, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues yellow to Donald");
    // Bob's slot 1 should become known r1.
    has_infs(&game, None, Bob, 1, &["r1"]);
    // Cathy's slot 1 should be finessed.
    has_infs(&game, None, Cathy, 1, &["r2"]);
    has_status(&game, Cathy, 1, CardStatus::Finessed);
}

#[test]
fn understands_a_prompt_finesse_2() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "y4", "b4", "p4"],
            &["r2", "y4", "g4", "r4"],
            &["g5", "b4", "r3", "y3"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Bob, 1, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues yellow to Donald");
    // Bob's slot 1 should become known r1.
    has_infs(&game, None, Bob, 1, &["r1"]);
    // Cathy's slot 1 should be finessed.
    has_infs(&game, None, Cathy, 1, &["r2"]);
    has_status(&game, Cathy, 1, CardStatus::Finessed);
}

#[test]
fn doesn_t_give_finesse_that_looks_like_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "r5", "g5", "p2", "p4"],
            &["r1", "r3", "p4", "r4", "p5"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues purple to Cathy");
    // This clue is nonsensical.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn understands_a_self_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r2", "b4", "g4", "p4"],
        ],
        Setup {
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    // Bob's slot 1 should be finessed.
    has_infs(&game, None, Bob, 1, &["r1"]);
    has_status(&game, Bob, 1, CardStatus::Finessed);
}

#[test]
fn doesn_t_give_self_finesses_that_look_direct() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r2", "b4", "g4", "p4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    // This clue is nonsensical.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_give_self_finesses_that_look_direct_2() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "b4", "p4", "g4"],
            &["r2", "r3", "g4", "r4", "p4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues green to Cathy");
    // This clue is nonsensical.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_give_self_finesse_that_looks_like_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "r5", "g5", "r3", "p4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
                pre_clue(g, Bob, 4, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    // This clue is nonsensical.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
