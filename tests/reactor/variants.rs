use super::common::*;
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn it_understands_a_playable_pink_promise() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r4", "y4", "y4"],
            &["g4", "g1", "g4", "b4", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 2, 1, 1, 2]),
            variant: TestVariant::Pink5,
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Alice (slots 2,4)");

    // Alice should play slot 2.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
    has_infs(&game, None, Alice, 2, &["r2", "g2", "b2"]);

    // Alice's slot 4 is not playable.
    let playables = game.common.obvious_playables(&game, Alice.ordinal());
    assert_eq!(playables, vec![game.state.hands[Alice.ordinal()][1]]);
}

#[test]
fn it_understands_a_brown_tcm() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r4", "y4", "y4"],
            &["g4", "g1", "g4", "b4", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 2, 1, 1, 2]),
            variant: TestVariant::Brown5,
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,4)");

    // Alice does not have a playable.
    assert!(
        game.common
            .obvious_playables(&game, Alice.ordinal())
            .is_empty()
    );
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn it_doesnt_brown_tcm_when_it_cant_be_saving_a_brown_on_chop() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r4", "y4", "y4"],
            &["g4", "g1", "g4", "b4", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 2, 1, 1, 2]),
            variant: TestVariant::Brown5,
            init: Box::new(|g| pre_clue(g, Alice, 5, &["1"])),
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,4)");

    // Since Alice had a safe action, this is a trash push.
    assert_eq!(
        game.common.obvious_playables(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
    has_status(&game, Alice, 1, CardStatus::DragToPlay);
}

#[test]
fn understands_a_brown_tempo_clue() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "r2", "g4", "b4", "n4"],
            &["r4", "y4", "g4", "b4", "n4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Brown5,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["brown"]);
                pre_clue(g, Alice, 4, &["brown"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues brown to Alice (slots 2,4)");

    // This is a tempo clue on slot 4.
    assert_eq!(
        game.common.obvious_playables(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
    has_status(&game, Alice, 4, CardStatus::CalledToPlay);
}

#[test]
fn it_uses_value_cluing_for_pink() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r4", "y4", "y4"],
            &["g4", "r4", "g4", "b4", "b4"],
        ],
        Setup {
            variant: TestVariant::Pink5,
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Bob");

    // 3 + 1 = 4, not 2 + 1 = 3
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
}

#[test]
fn understands_a_pink_tempo_clue() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "r2", "g4", "b4", "i4"],
            &["r4", "y4", "g4", "b4", "i4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["pink"]);
                pre_clue(g, Alice, 4, &["pink"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Alice (slots 2,4)");

    // This is a tempo clue on slot 4.
    assert_eq!(
        game.common.obvious_playables(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
    has_status(&game, Alice, 4, CardStatus::CalledToPlay);
}

#[test]
fn it_uses_value_cluing_for_rainbow() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "b1", "r4", "r4", "y4"],
            &["g4", "y4", "g4", "b4", "b4"],
        ],
        Setup {
            variant: TestVariant::Rainbow5,
            starting: Cathy,
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");

    // 5 + 1 = 1, not 2 + 1 = 3
    has_status(&game, Alice, 5, CardStatus::CalledToDiscard);
}
