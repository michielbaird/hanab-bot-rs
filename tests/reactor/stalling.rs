use super::common::*;
use scala_bot::basics::CardStatus;

use Player::*;

#[test]
fn it_understands_a_bad_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
            &["g1", "p4", "p4", "b4", "g4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues blue to Cathy");

    // Bob's slot 3 should be called to discard, as p4 is not playable.
    has_status(&game, Bob, 3, CardStatus::CalledToDiscard);
}

#[test]
fn it_doesnt_react_to_a_cathy_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
            &["g4", "p4", "p1", "b4", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues blue to Cathy");

    // Bob's slot 1 should not called to discard, as this is an allowable play clue on turn 1.
    has_status(&game, Bob, 1, CardStatus::PermissionToDiscard);
}

#[test]
fn it_reacts_to_cathy_1s() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g1"],
            &["g4", "p4", "p1", "b1", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");

    // Bob's slot 5 is called to play, since colour can be given to Cathy.
    has_status(&game, Bob, 5, CardStatus::DragToPlay);
}

#[test]
fn it_doesnt_react_to_untargetable_cathy_1s() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g1"],
            &["g4", "p4", "p1", "p3", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");

    // Bob's slot 5 is not called to play, since colour can't given to Cathy.
    has_status(&game, Bob, 5, CardStatus::None);
}

#[test]
fn it_reacts_when_a_direct_play_is_available() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "g5", "y4", "y1", "g4"],
            &["r3", "r3", "y3", "y3", "b3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Bob");

    // Even though green to Bob is unavailable, 1 to Bob is, so Alice should react.
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
}

#[test]
fn it_reacts_at_8_clues_when_giver_is_loaded() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "g5", "r3", "y4", "g4"],
            &["r1", "r3", "y3", "y3", "b3"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues blue to Cathy");
    take_turn_draw(&mut game, "Bob discards y4", "p1"); // Cathy loaded on r1
    take_turn(&mut game, "Cathy clues 5 to Bob");

    // Even though Cathy is at 8 clues, she is loaded, so Alice should react.
    has_status(&game, Alice, 2, CardStatus::DragToPlay);
}
