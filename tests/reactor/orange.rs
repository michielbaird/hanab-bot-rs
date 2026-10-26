use super::common::*;
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn calls_the_reacters_orange_playable_to_discard() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "o1", "b4", "b4", "r4"],
            &["g1", "g4", "r5", "g4", "y4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );

    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Cathy.ordinal(),
            value: Colour::Red.ordinal()
        }
    );
}

#[test]
fn calls_the_receivers_orange_playable_to_discard() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "o1", "b4", "b4", "r4"],
            &["r4", "g4", "r5", "g4", "y4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");

    // Alice should play slot 3.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
}

#[test]
fn gives_the_correct_clue_to_a_receiver_with_known_orange() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g4", "g1", "b4", "r4"],
            &["b4", "g4", "r5", "o1", "y4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            init: Box::new(|g| pre_clue(g, Cathy, 4, &["orange"])),
            ..Setup::default()
        },
    );

    // Alice should clue green to Cathy (Bob play + Cathy discard).
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Cathy.ordinal(),
            value: Colour::Green.ordinal()
        }
    );
}

#[test]
fn understands_when_an_orange_card_is_used_to_react() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "r4", "b4", "b4", "r4"],
            &["g4", "o1", "r5", "g4", "y4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy plays o1", "y4");

    has_status(&game, Alice, 2, CardStatus::DragToPlay);
}

#[test]
fn drags_a_known_orange_to_the_play_stacks_after_being_called_to_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r3", "g3", "g4", "r4"],
            &["b4", "y4", "r5", "g4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            init: Box::new(|g| pre_clue(g, Alice, 2, &["orange"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy discards y4", "y4");

    has_status(&game, Alice, 2, CardStatus::DragToPlay);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn drags_an_orange_target_of_a_ref_play_to_the_play_stacks() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "o4"],
            &["r3", "y3", "g3", "b3", "o3"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues orange to Alice (slots 1,2)");

    has_status(&game, Alice, 3, CardStatus::None);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn always_drags_a_trash_pushed_card_to_the_play_stacks() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "b1", "o1"],
            &["r2", "y2", "g2", "b2", "o2"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            play_stacks: Some(vec![5, 5, 5, 5, 2]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,3)");

    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn drags_the_target_of_an_orange_tempo_clue_to_the_discard_stacks() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "b1", "o1"],
            &["r1", "y1", "g1", "b1", "o1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            play_stacks: Some(vec![5, 5, 5, 5, 2]),
            clue_tokens: 7,
            init: Box::new(|g| pre_clue(g, Alice, 5, &["orange"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues orange to Alice (slot 5)");

    has_infs(&game, None, Alice, 5, &["o3"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn keeps_a_call_to_discard_in_dark_orange() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "o5", "b4"],
            &["r1", "y4", "g4", "y1", "o4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::DOrange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy plays r1", "b4");
    has_status(&game, Alice, 3, CardStatus::CalledToDiscard);
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues orange to Alice (slot 3)");
    take_turn_draw(&mut game, "Cathy plays y1", "y3");

    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);

    // Alice should discard the o1.
    has_infs(&game, None, Alice, 3, &["o1"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
}

#[test]
fn reacts_to_a_bad_stable_clue_calling_o1_to_be_dragged_to_the_play_stacks() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o1", "y4", "g4", "b1", "r4"],
            &["r4", "y4", "g4", "b4", "o4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob");

    // We should play slot 1 so that Bob drags o1 to the discard stacks.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}
