#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::interp::{ClueInterp, Interp};
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn gives_a_colour_clue_to_playable_slot_1() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "p4", "y5", "y4", "b2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: Colour::Blue.ordinal()
        }
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    has_infs(&game, None, Bob, 1, &["b1"]);
}

#[test]
fn gives_a_referential_discard_to_save_lh_ptd() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y5", "y3", "g3", "y1", "r4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    has_status(&game, Bob, 2, CardStatus::CalledToDiscard);
}

#[test]
fn understands_a_colour_stall() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p3", "y1", "b4", "r3", "g4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Alice clues green to Bob");
    has_status(&game, Bob, 4, CardStatus::None);
    has_status(&game, Bob, 1, CardStatus::PermissionToDiscard);
}

#[test]
fn doesn_t_get_locked_hand_ptd_on_a_known_playable() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "y3", "r5", "b5", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues yellow to Bob");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Alice clues 5 to Bob");
    has_status(&game, Bob, 1, CardStatus::DragToPlay);
}

#[test]
fn unlocks_a_directly_connecting_card() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "p4", "y5", "y4", "g2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 4,5)");
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob plays b1", "p1");
    has_infs(&game, None, Alice, 5, &["b2"]);
}

#[test]
fn unlocks_an_unclued_card() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "p4", "y5", "y4", "g2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob plays b1", "p1");
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][3])
    );
}

#[test]
fn unlocks_a_directly_connecting_card_after_a_shift() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "p4", "y5", "y4", "g2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 4,5)");
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob discards y4", "p1");
    take_turn(&mut game, "Alice clues purple to Bob");
    take_turn_draw(&mut game, "Bob plays b1", "b4");
    has_infs(&game, None, Alice, 4, &["b2"]);
}

#[test]
fn unlocks_after_shifting_beyond_all_directly_connecting_cards() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "p4", "y5", "y4", "g2"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Alice, 4, &["blue"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 3,5)");
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob discards y4", "r1"); // delay 1
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob discards p4", "b4"); // delay 2
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn_draw(&mut game, "Bob plays b1", "y3");
    // Shifted to slot 2.
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][1])
    );
}

#[test]
fn doesn_t_unlock_when_playing_oldest_playable_with_no_discard() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "p5", "y5", "y4", "b4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 4,5)");
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn_draw(&mut game, "Bob discards y4", "p1");
    take_turn(&mut game, "Alice clues purple to Bob");
    take_turn_draw(&mut game, "Bob plays b1", "b4");
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}

#[test]
fn doesn_t_play_if_won_t_unlock() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g5", "p2", "r2", "g4", "r5"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slot 3)");
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn_draw(&mut game, "Bob discards g4", "p5");
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues blue to Alice (slot 1)");
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][3]
        }
    );
}

#[test]
fn prefers_unlocking_over_discarding_trash() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g5", "p2", "r2", "g4", "r5"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 3,4)");
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn_draw(&mut game, "Bob discards g4", "p5");
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues red to Alice (slots 3,4)");
    assert!(matches!(game.take_action(), PerformAction::Play { .. }));
}
