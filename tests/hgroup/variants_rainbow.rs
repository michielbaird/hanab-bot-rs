#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::action::ClueAction;
use hanab_bot::basics::action::{Action, DiscardAction};
use hanab_bot::basics::clue::{BaseClue, ClueKind};
use hanab_bot::basics::clue_result::{bad_touch_result, playables_result};
use hanab_bot::basics::connection::Connection;
use hanab_bot::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use hanab_bot::basics::player::Link;
use hanab_bot::basics::{CardStatus, Identity, PerformAction};
use hanab_bot::collections::FastBitSet;
use hanab_bot::endgame::EndgameSolver;
use hanab_bot::frac::Frac;
use hanab_bot::hgroup::state_eval::eval_action_impl;
use hanab_bot::hgroup::state_eval::get_result;
use hanab_bot::hgroup::{DcStatus, FStatus, HGroupGame, StallInterp};

use Player::*;

#[test]
fn understands_a_delayed_play_through_possible_rainbow_ids() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "r1", "b2", "y3"],
            &["r2", "y4", "b3", "r3"],
            &["y4", "m2", "r4", "g1"],
        ],
        Setup {
            variant: TestVariant::Rainbow5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Donald"); // getting m2
    take_turn(&mut game, "Bob clues green to Donald"); // getting g1 (note [g1,m3])
    take_turn(&mut game, "Cathy clues green to Bob"); // getting g2
    // Alice shouldn't be finessed for g1.
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn connects_through_own_unknown_cards() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "y2", "r2", "r3", "m3"],
            &["y4", "b3", "b4", "r1", "b2"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            variant: TestVariant::Rainbow5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,4)");
    take_turn(&mut game, "Alice plays g1 (slot 2)");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 2,3)"); // xx x2 x2 x1 xx
    take_turn(&mut game, "Cathy clues yellow to Alice (slots 1,2,4)");
    // Only slot 4 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
    let mut yellow = game.clone();
    take_turn(&mut yellow, "Alice plays y1 (slot 4)");
    has_infs(&yellow, None, Alice, 2, &["y3", "m1"]);
    let mut rainbow = game.clone();
    take_turn(&mut rainbow, "Alice plays m1 (slot 4)");
    has_infs(&rainbow, None, Alice, 2, &["y1", "m3"]);
}

#[test]
fn doesn_t_consider_a_playable_connection_to_be_a_prompt() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g1", "m1", "m1"],
            &["b4", "y2", "r2", "r3"],
            &["y4", "g3", "m2", "r4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 2, 0, 1]),
            variant: TestVariant::Rainbow5,
            init: Box::new(|g| {
                pre_clue(g, Donald, 3, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Donald");
    has_infs(&game, None, Donald, 2, &["g3", "m3"]);
    take_turn(&mut game, "Cathy clues 4 to Alice (slot 4)");
    let g4 = game.state.expand_short("g4").unwrap();
    let wc = game
        .waiting()
        .iter()
        .find(|wc| wc.target == game.state.hands[Alice.ordinal()][3] && wc.inference == g4);
    // Not a prompt. Note that a waiting connection may not be generated for playable connections only.
    assert!(wc.is_none_or(|w| {
        !w.connections
            .iter()
            .any(|c| matches!(c, Connection::Prompt { .. }))
    }));
}

#[test]
fn plays_into_a_free_choice_prompt() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "b3", "m2"],
            &["y4", "y4", "y3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    has_infs(&game, None, Alice, 2, &["m1"]);
}

#[test]
fn plays_into_a_free_choice_finesse() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g3", "b3", "b3", "m2"],
            &["y4", "y4", "y3", "y3", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["m1"]);
}

#[test]
fn assumes_a_free_choice_finesse_over_direct_play() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["m1", "r3", "y3", "g3", "m5"],
            &["r4", "y4", "g4", "b4", "m4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["green"]);
                pre_clue(g, Bob, 5, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Alice (slot 2)"); // filling in slot 2 as a rainbow card
    // Alice should allow for a possible free choice finesse.
    has_infs(&game, None, Alice, 2, &["m1", "m2"]);
}

#[test]
fn prompts_when_giver_doesn_t_have_free_choice_1() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "m2"],
            &["r4", "y4", "g4", "b4", "m4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    // Since any clue will touch more than just the rainbow card,
    // Cathy doesn't have free choice and they all prompt Alice.
    for clue in ["red", "yellow", "green", "blue"] {
        let mut hypo = game.clone();
        take_turn(&mut hypo, &format!("Cathy clues {clue} to Bob"));
        has_status(&hypo, Alice, 1, CardStatus::None);
        has_infs(&hypo, None, Alice, 3, &["m1"]);
    }
}

#[test]
fn prompts_when_giver_doesn_t_have_free_choice_2() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "g4", "m2"],
            &["y4", "y4", "b4", "b4", "m4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    // Although yellow and blue only touch the rainbow card,
    // red doesn't, so Cathy doesn't have free choice.
    for clue in ["red", "yellow", "green", "blue"] {
        let mut hypo = game.clone();
        take_turn(&mut hypo, &format!("Cathy clues {clue} to Bob"));
        has_status(&hypo, Alice, 1, CardStatus::None);
        has_infs(&hypo, None, Alice, 3, &["m1"]);
    }
}

#[test]
fn prompts_when_giver_doesn_t_use_free_choice() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b5", "b4", "g4", "g4", "m2"],
            &["y4", "y4", "r4", "r4", "m4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Rainbow5,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    // Although red and yellow only touch the rainbow card,
    // Cathy's clue touches non-rainbow cards, so it's not a free choice finesse.
    has_status(&game, Alice, 1, CardStatus::None);
    has_infs(&game, None, Alice, 3, &["m1"]);
}
