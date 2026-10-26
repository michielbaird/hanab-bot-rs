#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn calls_an_orange_playable_to_discard() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "o1", "b4", "b4", "r4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 3
        }
    );
}

#[test]
fn calls_orange_unplayables_to_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o4", "r3", "g3", "b3", "y3"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Alice (slot 2)");
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: Colour::Red.ordinal()
        }
    );
}

#[test]
fn lets_a_playable_orange_card_discard() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o1", "r3", "g3", "b3", "y3"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Alice (slot 2)");
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn drags_an_orange_target_of_a_ref_play_to_the_play_stacks() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues orange to Alice (slots 1,2)");
    has_status(&game, Alice, 3, CardStatus::None);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn gives_an_orange_fix_clue_on_a_rank_playable_card() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "r1", "o1"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "y4");
    // Alice needs to prevent Bob from discarding the o1.
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: 4
        }
    );
}

#[test]
fn drags_to_the_play_stacks_even_when_the_only_playables_are_orange() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "y1", "y1", "g1"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![5, 5, 5, 5, 0]),
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 2)");
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}
