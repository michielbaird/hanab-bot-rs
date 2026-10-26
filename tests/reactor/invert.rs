use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn it_reacts_to_a_response_inversion() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "y5", "g4", "b4", "b4"],
            &["b1", "r1", "r4", "y4", "y4"],
        ],
        Setup {
            starting: Cathy,
            init: Box::new(|g| pre_clue(g, Alice, 5, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");
    has_status(&game, Alice, 2, CardStatus::CalledToDiscard);

    // We should discard slot 2 urgently.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
    take_turn(&mut game, "Alice discards r4 (slot 2)");

    has_status(&game, Bob, 1, CardStatus::DragToPlay);
}

#[test]
fn it_receives_a_response_inversion() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y5", "g4", "b4", "b4"],
            &["y4", "r1", "r4", "y4", "y1"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| pre_clue(g, Cathy, 5, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 4)");
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Cathy plays r1", "r4");

    has_status(&game, Alice, 2, CardStatus::CalledToDiscard);
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn it_doesnt_receive_a_declined_inversion_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y5", "g4", "b4", "b4"],
            &["y4", "r1", "r4", "y4", "y1"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| pre_clue(g, Cathy, 5, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 4)");
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Cathy plays y1", "r4");

    has_status(&game, Alice, 4, CardStatus::None);
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
}

#[test]
fn it_doesnt_receive_a_declined_inversion_discard_from_chop() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y5", "g4", "b4", "b3"],
            &["y4", "r1", "r4", "y4", "y1"],
        ],
        Setup::default(),
    );
    // Lock Bob
    take_turn(&mut game, "Alice clues 3 to Bob");
    take_turn(&mut game, "Bob clues 3 to Alice (slot 3)");
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
    take_turn_draw(&mut game, "Cathy discards y4 (slot 1)", "r4");

    has_status(&game, Alice, 2, CardStatus::None);
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
}

#[test]
fn it_doesnt_receive_a_declined_inversion_discard_from_kt() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y5", "g4", "b4", "b3"],
            &["y4", "r1", "r4", "y4", "y1"],
        ],
        Setup {
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            init: Box::new(|g| fully_known(g, Cathy, 5, "y1")),
            ..Setup::default()
        },
    );
    // Lock Bob
    take_turn(&mut game, "Alice clues 3 to Bob");
    take_turn(&mut game, "Bob clues 3 to Alice (slot 3)");
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
    take_turn_draw(&mut game, "Cathy discards y1", "r4");

    has_status(&game, Alice, 3, CardStatus::None);
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
}

#[test]
fn it_receives_an_inverted_dc_when_the_reacter_is_loaded() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y5", "g4", "b4", "b3"],
            &["y4", "r1", "r4", "y4", "y1"],
        ],
        Setup {
            clue_tokens: 7,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                fully_known(g, Cathy, 2, "r1");
                fully_known(g, Cathy, 5, "y1");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    take_turn(&mut game, "Bob clues red to Alice (slot 3)"); // Since Cathy is loaded, this is stable
    has_status(&game, Alice, 2, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Cathy discards r1 (slot 2)", "r4"); // Even though this is kt, Cathy is expected to play

    has_status(&game, Alice, 1, CardStatus::DragToPlay);
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn it_doesnt_give_a_response_inversion_to_dc_kt_when_reactor_is_unloaded() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r1", "g4", "b4", "b3"],
            &["p5", "y1", "r4", "g3", "y4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| fully_known(g, Bob, 2, "r1")),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)"); // Lock Alice
    take_turn(&mut game, "Alice clues green to Cathy");

    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn it_gives_a_response_inversion_to_dc_kt_when_reacter_is_loaded() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r1", "g4", "b4", "b1"],
            &["p5", "y1", "r4", "g3", "y4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                fully_known(g, Bob, 2, "r1");
                fully_known(g, Bob, 5, "b1");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)"); // Lock Alice
    take_turn(&mut game, "Alice clues green to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Reactive)));
    take_turn_draw(&mut game, "Bob discards r1", "r3");

    // Since Bob was expected to play b1, discarding r1 is a reaction.
    has_status(&game, Cathy, 2, CardStatus::DragToPlay);
}

#[test]
fn it_understands_a_bad_lock() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y5", "g1", "b4", "b1"],
            &["y4", "r1", "b4", "y4", "y3"],
        ],
        Setup {
            init: Box::new(|g| pre_clue(g, Bob, 5, &["1"])),
            ..Setup::default()
        },
    );
    // Blue is available to push Cathy's r1.
    take_turn(&mut game, "Alice clues 3 to Cathy");
    has_status(&game, Bob, 3, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Bob plays g1", "r4");

    has_status(&game, Cathy, 2, CardStatus::DragToPlay);
    assert!(!game.common.obvious_locked(&game, Cathy.ordinal()));
}
