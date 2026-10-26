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
fn plays_into_a_self_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "p4", "b4", "b4", "y4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 1)");
    let g = &game;
    has_status(g, Alice, 2, CardStatus::Finessed);
    take_turn(&mut game, "Alice plays g1 (slot 2)");
    has_infs(&game, None, Alice, 2, &["g2"]);
}

#[test]
fn doesn_t_give_a_bad_self_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "p4", "b4", "b4", "y4"],
            &["g3", "g2", "r4", "r4", "y4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");
    take_turn(&mut game, "Alice clues 3 to Cathy");
    // This clue is illegal.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn interprets_a_self_finesse_when_giver_knows_less() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "r1", "g4", "b1", "g3"],
            &["g1", "g2", "r5", "y3", "p3"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn(&mut game, "Bob clues 2 to Cathy");
    has_status(&game, Cathy, 1, CardStatus::Finessed);
}

#[test]
fn interprets_a_self_finesse_when_other_possibilities_are_impossible() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "b4", "g4", "p1"],
            &["b2", "b3", "p1", "g3"],
            &["b5", "b2", "r4", "p5"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues purple to Alice (slot 2)"); // p1 play
    take_turn(&mut game, "Alice plays p1 (slot 2)");
    take_turn(&mut game, "Bob clues blue to Cathy"); // b1 reverse on us
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues 4 to Bob"); // connect b4
    take_turn(&mut game, "Alice plays b1 (slot 1)"); // b1, p1 now played
    take_turn(&mut game, "Bob clues 2 to Alice (slot 3)"); // neg purple, b2 is clued
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 3, &["r2", "y2", "g2"]);
}

#[test]
fn doesn_t_give_a_self_finesse_that_looks_like_a_prompt() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "b4", "g4", "p1", "b2"],
            &["y2", "b2", "r5", "y3", "y4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Cathy, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Cathy");
    // This clue is illegal, since r5 will prompt as r2.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn gives_a_self_finesse_that_doesn_t_look_like_a_prompt() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "b3", "g1", "p1", "y2"],
            &["g2", "b3", "p1", "g3", "b2"],
        ],
        Setup {
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    take_turn_draw(&mut game, "Bob plays r3", "g3");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Alice clues 3 to Bob");
    // g1 self-finesse, g2 finesse on Cathy
    has_status(&game, Bob, 3, CardStatus::Finessed);
    has_status(&game, Cathy, 1, CardStatus::Finessed);
}

#[test]
fn doesn_t_give_a_self_finesse_that_isn_t_the_simplest_interpretation() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "p3", "r3", "g3"],
            &["p3", "g5", "p4", "g2"],
            &["b1", "b2", "p1", "g4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Cathy");
    take_turn(&mut game, "Alice clues 3 to Bob");
    // This clue is illegal, since the focus will look like b3.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn maintains_a_self_finesse_even_as_inferences_are_reduced() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "b4", "b1", "g1"],
            &["r1", "r3", "r1", "b4"],
            &["y1", "r4", "p3", "g1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays y1", "p4");
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");
    let g = &game;
    // All of these are valid self-finesses.
    // hasInfs(g, None, Alice, 1, Vector("r1", "y2", "g2", "b2", "p1"))
    has_status(g, Alice, 1, CardStatus::Finessed);
    take_turn(&mut game, "Donald clues green to Alice (slot 4)");
    // After knowing we have g2 in slot 4, the finesse should still be on.
    has_infs(&game, None, Alice, 4, &["g2"]);
    // hasInfs(game, None, Alice, 1, Vector("r1", "y2", "b2", "p1"))
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_an_asymmetric_self_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "r5", "y3", "g4"],
            &["r4", "b5", "p4", "g3"],
            &["p3", "b4", "g3", "b3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![2, 0, 0, 4, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 5 to Bob");
    // Donald, Bob and Alice can see b5, so we all know this is an r5 finesse.
    // hasInfs(game, None, Bob, 2, Vector("r5"))
    has_status(&game, Bob, 1, CardStatus::Finessed);
}

#[test]
fn plays_even_if_it_could_be_an_asymmetric_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "r5", "y3", "g4"],
            &["r4", "y5", "p4", "g3"],
            &["p3", "b4", "g3", "b3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![2, 0, 0, 4, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 5 to Bob");
    // We must play into this, since we can't assume we have b5.
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn prefers_to_self_finesse_over_assuming_asymmetric_information() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r1", "y2", "r2", "b4"],
            &["r3", "g3", "b2", "b3", "m4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 2, 0, 3]),
            discarded: &["r3", "m4"],
            variant: TestVariant::Rainbow5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Cathy"); // r1, r3, m4
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    // Alice's slot 1 should be finessed as g3.
    // hasInfs(game, None, Alice, 1, Vector("y1", "g3", "b1"))
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn realizes_a_self_finesse_after_other_possibilities_are_stomped() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y4", "r5", "y3", "g4"],
            &["r2", "g4", "p1", "g1"],
            &["r3", "b4", "g1", "b2"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Cathy, 3, &["1"]);
                pre_clue(g, Cathy, 4, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 4 to Alice (slot 3)");
    take_turn(&mut game, "Alice discards y3 (slot 4)");
    take_turn(&mut game, "Bob clues red to Donald"); // finessing r2, proving !r
    has_infs(&game, None, Alice, 4, &["y4", "g4", "p4"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn understands_a_very_delayed_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p4", "y4", "g1", "r4"],
            &["b1", "g1", "y2", "y2"],
            &["b3", "y1", "p1", "p3"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");
    take_turn(&mut game, "Bob clues 3 to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy plays g1", "y5");
    take_turn(&mut game, "Donald clues yellow to Bob");
    take_turn(&mut game, "Alice clues 5 to Cathy"); // 5 Stall
    take_turn(&mut game, "Bob clues red to Alice (slot 3)");
    has_infs(&game, None, Alice, 3, &["r1"]);
    take_turn_draw(&mut game, "Cathy plays b1", "g4");
    take_turn(&mut game, "Donald clues 5 to Alice (slot 2)");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    // The finesse is revealed to be yellow.
    has_infs(&game, None, Alice, 4, &["y3"]);
}

#[test]
fn understands_a_fake_self_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "y5", "b4", "y3", "r1"],
            &["r4", "g5", "p4", "g3", "b5"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 1, 1, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues 3 to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "b3");
    // Bob needs to play r1 first to respect r3.
    has_status(&game, Bob, 2, CardStatus::Finessed);
    has_infs(&game, None, Bob, 5, &["r3", "y3", "g3"]);
}

#[test]
fn knows_when_a_self_finesse_is_never_possible() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "y5", "b4", "y3", "r1"],
            &["r4", "g5", "p4", "g3", "b5"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["y4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["y4"]);
    assert!(game.waiting().is_empty());
}

#[test]
fn self_finesses_rather_than_assuming_asymmetric_information() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p2", "r4", "y4", "g4", "b4"],
            &["p1", "r4", "y4", "g4", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn(&mut game, "Bob clues 3 to Alice (slot 5)");
    let g = &game;
    // Alice should be finessed, rather than assuming Bob knows p2.
    has_status(g, Alice, 1, CardStatus::Finessed);
    take_turn_draw(&mut game, "Cathy plays p1", "y3");
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn doesn_t_consider_self_finesses_when_it_could_be_a_save() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "r4", "y4", "g4", "b4"],
            &["p4", "r4", "y4", "g4", "b4"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["r3"],
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["r3", "p3"]);
}

#[test]
fn isn_t_locked_by_a_potentially_ambiguous_self_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "r3", "y3", "g3", "b3"],
            &["p4", "r4", "y4", "g4", "b4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Alice (slots 4,5)");
    // Even though Alice could have [r1, r2, r3, ?4, r4], she can't think she's locked.
    assert!(!game.common.thinks_locked(&game, Alice.ordinal()));
}

#[test]
fn prefers_to_interpret_a_prompt_self_finesse_over_a_prompt_reverse_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p4"],
            &["r3", "r4", "y3", "r2", "b3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Cathy, 4, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Cathy");
    has_status(&game, Cathy, 1, CardStatus::Finessed);
    has_status(&game, Alice, 1, CardStatus::None);
}
