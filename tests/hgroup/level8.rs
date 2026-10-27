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
fn plays_from_a_pos_dc() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r2", "r3", "b1"],
            &["b1", "b2", "g1", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Cathy discards g1", "p4");
    // Alice's slot 3 should be called to play from a positional discard.
    has_status(&game, Alice, 3, CardStatus::CalledToPlay);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
}

#[test]
fn doesn_t_play_from_a_pos_dc_to_someone_after_them() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "p5", "r3", "b1"],
            &["b1", "b2", "g1", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Cathy discards g1", "p4");
    // Bob's slot 3 should be called to play from a positional discard.
    has_status(&game, Bob, 3, CardStatus::CalledToPlay);
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn doesn_t_play_from_a_pos_dc_if_someone_before_them_plays_into_it() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r2", "r3", "b1"],
            &["b1", "b2", "p5", "g1", "p1"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Bob discards r2", "p4");
    let g = &game;
    // Cathy's slot 3 should be called to play from a positional discard.
    has_status(g, Cathy, 3, CardStatus::CalledToPlay);
    take_turn_draw(&mut game, "Cathy plays p5", "y5");
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn plays_into_a_pos_dc_if_someone_before_them_doesn_t_play_into_it() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r2", "r3", "b1"],
            &["b1", "b2", "p5", "g1", "p1"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Bob discards r2", "p4");
    take_turn_draw(&mut game, "Cathy discards p1", "y1");
    has_status(&game, Alice, 3, CardStatus::CalledToPlay);
}

#[test]
fn doesn_t_play_from_a_chop_discard() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "p5", "r3", "b1"],
            &["b1", "b2", "g1", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Cathy discards p1", "p4");
    has_status(&game, Alice, 5, CardStatus::None);
}

#[test]
fn doesn_t_play_from_a_trash_discard_in_the_correct_order() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "p5", "r3", "b1"],
            &["b1", "b2", "g1", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 3, &["1"]);
                pre_clue(g, Cathy, 5, &["1"]);
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    assert_eq!(game.state.cards_left, 2);
    take_turn_draw(&mut game, "Cathy discards g1", "p4");
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn doesn_t_play_when_discarding_chop_from_multiple_unclued_trash() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "p5", "r3", "b1"],
        ],
        Setup {
            play_stacks: Some(vec![5, 5, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b1", "b2", "b3", "b4", "p1", "p2",
                "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 3);
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn_draw(&mut game, "Bob plays p5", "r4");
    take_turn(&mut game, "Alice discards y1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b1", "b5");
    has_status(&game, Alice, 5, CardStatus::PermissionToDiscard);
}

#[test]
fn recognizes_a_pos_dc_on_the_last_player() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "r1", "b5", "b1"],
            &["b1", "b2", "g5", "g1"],
            &["g1", "g3", "r3", "p4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![4, 4, 4, 4, 5]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g2", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Donald discards r3", "r5");
    // Cathy's slot 3 should be called to play, while Bob's should not.
    has_status(&game, Bob, 3, CardStatus::None);
    has_status(&game, Cathy, 3, CardStatus::CalledToPlay);
}

#[test]
fn performs_a_pos_dc() {
    let game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "r1", "b5", "b1"],
            &["b1", "b2", "r2", "g1"],
            &["g1", "g3", "r3", "p4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![5, 5, 5, 4, 5]),
            discarded: &["y1", "y2", "y3", "y4", "g2", "g4", "b3", "b4", "p1", "p2"],
            clue_tokens: 0,
            init: Box::new(|g| {
                g.state.endgame_turns = Some(4);
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    assert_eq!(game.state.cards_left, 0);
    // Alice should discard slot 3 as a positional discard.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
}

#[test]
fn plays_from_a_pos_dc_against_common_good_touch() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "b1", "r2", "i1"],
            &["b1", "r1", "g1", "g1"],
            &["r1", "r5", "i1", "y1"],
        ],
        Setup {
            play_stacks: Some(vec![4, 4, 5, 5, 5]),
            discarded: &["y2", "y3", "y4", "g2", "g4", "b3", "b4", "i2", "i3"],
            clue_tokens: 2,
            variant: TestVariant::Pink5,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn(&mut game, "Alice clues 5 to Donald");
    take_turn(&mut game, "Bob clues red to Donald");
    take_turn_draw(&mut game, "Cathy discards r1", "r3");
    // Alice's slot 2 is called to play.
    has_status(&game, Alice, 2, CardStatus::CalledToPlay);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn doesn_t_bomb_from_a_mistake() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b5", "b4"],
            &["g2", "b3", "r5", "p2", "p3"],
        ],
        Setup {
            play_stacks: Some(vec![4, 5, 5, 4, 5]),
            discarded: &[
                "r1", "r2", "r3", "y1", "y2", "y3", "g1", "g3", "b1", "b2", "p1",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 1);
    take_turn(&mut game, "Alice clues red to Cathy");
    take_turn_draw(&mut game, "Bob discards g4", "r1");
    // Alice should not attempt to play.
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn doesn_t_bomb_from_a_mistake_2() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b5", "b4"],
            &["g2", "b3", "r5", "p2", "p3"],
        ],
        Setup {
            play_stacks: Some(vec![4, 5, 5, 4, 5]),
            discarded: &[
                "r1", "r2", "r3", "y1", "y2", "y3", "g1", "g3", "b1", "b2", "p1",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 1);
    take_turn(&mut game, "Alice clues red to Cathy");
    take_turn_draw(&mut game, "Bob discards g4", "r1");
    // Alice should not attempt to play.
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn prefers_to_positional_discard_rather_than_clue_for_the_last_card() {
    let game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["n4", "n5", "r4", "b2", "r3"],
        ],
        Setup {
            play_stacks: Some(vec![5, 5, 5, 5, 4]),
            discarded: &[
                "r1", "r1", "y1", "y1", "y2", "y4", "g1", "g3", "b1", "b1", "b3", "n1", "n1", "n2",
                "n3",
            ],
            clue_tokens: 3,
            variant: TestVariant::Brown5,
            ..Setup::default()
        },
    );
    assert_eq!(game.state.cards_left, 1);
    let (perform, winrate) = EndgameSolver::new(false)
        .with_timeout(std::time::Duration::from_secs(60))
        .solve(&game, None)
        .unwrap_or_else(|msg| panic!("Game should be winnable! {msg}"));
    assert_eq!(winrate, Frac::ONE);
    assert_eq!(
        perform,
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn plays_from_a_pos_misplay() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r5", "r3", "b1"],
            &["b1", "b2", "g1", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 0,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Cathy bombs p1", "p4");
    // Alice's slot 5 should be called to play from a positional misplay.
    has_status(&game, Alice, 5, CardStatus::CalledToPlay);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn plays_from_a_double_pos_misplay() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r5", "r3", "b1"],
            &["b1", "b2", "g1", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 4, 4, 4]),
            discarded: &[
                "y1", "y2", "y3", "y4", "g1", "g2", "g3", "g4", "b3", "b4", "p1", "p2", "p3",
            ],
            clue_tokens: 0,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 2);
    take_turn_draw(&mut game, "Cathy bombs g1", "p4");
    // Alice's slot 3 should be called to play from a positional misplay.
    has_status(&game, Alice, 3, CardStatus::CalledToPlay);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
}

#[test]
fn understands_a_distribution_clue() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "y4", "r4", "y1", "b1"],
            &["b4", "b3", "r1", "p2", "p3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![4, 5, 5, 3, 5]),
            discarded: &[
                "r1", "r2", "r3", "y1", "y2", "y3", "g1", "g3", "b1", "b2", "p1",
            ],
            clue_tokens: 1,
            init: Box::new(|g| {
                fully_known(g, Alice, 3, "r5");
                fully_known(g, Alice, 4, "b4");
                fully_known(g, Alice, 5, "b5");
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    assert_eq!(game.state.cards_left, 2);
    take_turn(&mut game, "Bob clues 4 to Cathy");
    has_infs(&game, None, Cathy, 1, &["b4"]);
    assert_eq!(
        game.last_move(),
        Some(Interp::Clue(ClueInterp::Distribution))
    );
}
