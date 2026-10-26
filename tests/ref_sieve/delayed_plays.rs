#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn gives_a_delayed_play_clue_through_a_playable_card() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y4", "y4", "g4"],
            &["b2", "b4", "p4", "p4", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues blue to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
}

#[test]
fn gives_a_delayed_play_clue_through_the_leftmost_playable_card() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "y4", "y4", "g4"],
            &["b2", "b4", "p4", "p4", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues blue to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
}

#[test]
fn doesn_t_give_a_delayed_play_clue_through_a_non_leftmost_playable_card() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "y4", "y4", "g4"],
            &["r2", "b4", "p4", "p4", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues blue to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn understands_a_self_delayed_play_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 3)"); // slot 2 called to play
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn(&mut game, "Bob clues yellow to Alice (slots 1,2)"); // slot 2 is known y1, slot 1 called to play
    has_infs(&game, None, Alice, 1, &["y2"]);
    has_infs(&game, None, Alice, 2, &["y1"]);
}
