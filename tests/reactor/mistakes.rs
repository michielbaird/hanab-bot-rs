use super::common::*;
use scala_bot::basics::CardStatus;

use Player::*;

#[test]
fn it_cancels_a_missed_reaction_1() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "r1", "g4", "b4", "b4"],
            &["b1", "r3", "r4", "y4", "y4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 4 to Cathy");
    // Bob is called to play r1 (slot 2) -> Cathy plays b1 (slot 1).
    has_status(&game, Bob, 2, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Bob discards g1", "y3");

    let bob_s2 = game.state.hands[Bob.ordinal()][1];

    // Bob is no longer called to play r1, and that card can be anything.
    assert_eq!(game.meta[bob_s2].status, CardStatus::None);
    assert_eq!(
        game.common.thoughts[bob_s2].inferred.len(),
        game.common.thoughts[bob_s2].possible.len()
    );

    // Cathy is not called to play slot 1 (Cathy might have some wrong priority elim notes).
    has_status(&game, Cathy, 1, CardStatus::None);
}

#[test]
fn it_cancels_a_missed_reaction_2() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "r1", "g4", "b4", "b4"],
            &["b1", "r1", "r4", "y4", "y4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 4 to Cathy");
    // Bob is called to play r1 (slot 2) -> Cathy plays b1 (slot 1).
    has_status(&game, Bob, 2, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Bob plays g1", "y3");

    let bob_s2 = game.state.hands[Bob.ordinal()][1];

    // Bob is no longer called to play r1, and that card can be anything.
    assert_eq!(game.meta[bob_s2].status, CardStatus::None);
    assert_eq!(
        game.common.thoughts[bob_s2].inferred.len(),
        game.common.thoughts[bob_s2].possible.len()
    );

    // Cathy is not called to play slot 1 (Cathy might have some wrong priority elim notes).
    has_status(&game, Cathy, 1, CardStatus::None);
}

#[test]
fn it_resets_ctd_after_a_slot_1_discard() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "r1", "b4", "b4"],
            &["r3", "b5", "r4", "y4", "y4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues blue to Cathy");
    take_turn_draw(&mut game, "Bob plays r1", "p5"); // 3+4 = 2
    has_status(&game, Cathy, 4, CardStatus::CalledToDiscard);
    take_turn_draw(&mut game, "Cathy discards r3", "y3");

    // Since Cathy demonstrated she doesn't know, remove the dc note.
    has_status(&game, Cathy, 4, CardStatus::None);
}
