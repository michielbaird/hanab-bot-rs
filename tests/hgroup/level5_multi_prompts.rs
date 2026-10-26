#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::action::ClueAction;
use scala_bot::basics::action::{Action, DiscardAction};
use scala_bot::basics::clue::{BaseClue, ClueKind};
use scala_bot::basics::clue_result::{bad_touch_result, playables_result};
use scala_bot::basics::connection::Connection;
use scala_bot::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use scala_bot::basics::player::Link;
use scala_bot::basics::{CardStatus, Identity, PerformAction};
use scala_bot::collections::FastBitSet;
use scala_bot::endgame::EndgameSolver;
use scala_bot::frac::Frac;
use scala_bot::hgroup::state_eval::eval_action_impl;
use scala_bot::hgroup::state_eval::get_result;
use scala_bot::hgroup::{DcStatus, FStatus, HGroupGame, StallInterp};

use Player::*;

#[test]
fn doesn_t_give_a_wrong_prompt_in_rainbow() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "m3", "r1", "r3"],
            &["g4", "g2", "b3", "m5"],
            &["b1", "y2", "r3", "r4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 0, 2, 0]),
            variant: TestVariant::Rainbow5,
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["blue"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    // m3 will bomb as prompt into m2.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn understands_a_fill_in_prompt_in_omni() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "o5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            variant: TestVariant::Omni5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 2,3,5)");
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues red to Alice (slots 4,5)");
    // The focus is !2 and !omni, so it must be r3 with r2 prompt in slot 5.
    has_infs(&game, None, Alice, 4, &["r3"]);
    has_infs(&game, None, Alice, 5, &["r2"]);
}

#[test]
fn prompts_the_card_with_the_most_positive_information_in_rainbow() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "r4"],
            &["y4", "y4", "b3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["red"]);
                pre_clue(g, Alice, 2, &["red", "3"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    // Alice should prompt slot 2 instead of slot 1.
    has_infs(&game, None, Alice, 2, &["r3"]);
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][0]]
            .inferred
            .len()
            > 1
    );
}

#[test]
fn prompts_leftmost_when_known_rainbow() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "m4"],
            &["y4", "y4", "b3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["red", "green"]);
                pre_clue(g, Alice, 2, &["red", "green", "blue"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    // Alice should prompt slot 1.
    has_infs(&game, None, Alice, 1, &["m3"]);
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][1]]
            .inferred
            .len()
            > 1
    );
}

#[test]
fn prompts_leftmost_when_good_touch_rainbow() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "m4"],
            &["y4", "y4", "b3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            play_stacks: Some(vec![5, 0, 0, 0, 2]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["red"]);
                pre_clue(g, Alice, 2, &["red", "green", "blue"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    // Alice should prompt slot 1.
    has_infs(&game, None, Alice, 1, &["m3"]);
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][1]]
            .inferred
            .len()
            > 1
    );
}

#[test]
fn prompts_leftmost_when_known_pink() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "i4"],
            &["y4", "y4", "b3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["pink"]);
                pre_clue(g, Alice, 2, &["pink", "3"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 1.
    has_infs(&game, None, Alice, 1, &["i3"]);
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][1]]
            .inferred
            .len()
            > 1
    );
}

#[test]
fn prompts_leftmost_when_known_omni() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "o4"],
            &["y4", "y4", "b3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Omni5,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["red", "green"]);
                pre_clue(g, Alice, 2, &["red", "green", "blue", "3", "5"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    // Alice should prompt slot 1.
    has_infs(&game, None, Alice, 1, &["o3"]);
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][1]]
            .inferred
            .len()
            > 1
    );
}

#[test]
fn prompts_the_card_with_most_positive_information_in_pink() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "y3", "i4"],
            &["y4", "y4", "b3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["3"]);
                pre_clue(g, Alice, 2, &["3", "5"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 2.
    has_infs(&game, None, Alice, 2, &["i3"]);
    assert!(
        game.common.thoughts[game.state.hands[Alice.ordinal()][0]]
            .inferred
            .len()
            > 1
    );
}
