use super::common::*;
use scala_bot::reactor::ReactorGame;

use Player::*;

#[test]
fn it_recognizes_ptd() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "y4", "b4", "b4", "p4"],
            &["y1", "r4", "r4", "g4", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "b5");

    // Alice has ptd.
    assert!(!game.must_clue());
}

#[test]
fn it_recognizes_loaded_ptd() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p5", "r4", "y1", "g4", "g4"],
            &["r5", "b5", "y4", "y3", "p4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    take_turn(&mut game, "Alice plays r1 (slot 4)"); // targeting Bob's trash in slot 3
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn_draw(&mut game, "Cathy discards y4", "b4");

    // Since Bob is loaded on y1 dc, Alice has ptd.
    assert!(!game.must_clue());
}

#[test]
fn it_recognizes_ptd_when_bob_has_trash_on_chop() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "y4", "b4", "b4", "p4"],
            &["y1", "r4", "r4", "g4", "g4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "b5");

    assert!(!game.must_clue());
}

#[test]
fn it_recognizes_no_ptd_when_bob_has_a_critical_on_chop() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y5", "y4", "b4", "b4", "p4"],
            &["y1", "r4", "r4", "g4", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "b5");

    assert!(game.must_clue());
}

#[test]
fn it_recognizes_no_ptd_when_bob_has_a_duplicated_2_on_chop() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p2", "y4", "b4", "b4", "p4"],
            &["y1", "r4", "r4", "g4", "p2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "b5");

    assert!(game.must_clue());
}

#[test]
fn it_recognizes_no_ptd_when_bob_has_a_copy_of_zeldas_unknown_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "y4", "b4", "b4", "p4"],
            &["y1", "r4", "r4", "g4", "p2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "b5");

    assert!(game.must_clue());
}
