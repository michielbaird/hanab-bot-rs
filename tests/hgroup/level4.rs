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
fn interprets_a_rank_tcm_for_1_card() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "b1", "r1", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Discard)));
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn doesn_t_tcm_if_chop_is_trash() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "b1", "b4", "g2"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_tcm_if_chop_is_duplicated() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "b1", "g4", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_tcm_if_chop_can_be_clued_directly_critical() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "b1", "r1", "g5"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_tcm_if_chop_can_be_clued_directly_playable() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "b1", "r1", "g3"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn recognizes_a_delayed_tcm_on_other() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "y4", "y1", "r3", "y3"],
            &["b4", "b4", "g4", "r1", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 2,3)");
    take_turn(&mut game, "Alice plays r4 (slot 2)");
    take_turn(&mut game, "Bob clues red to Cathy");
    has_status(&game, Cathy, 5, CardStatus::ChopMoved);
}

#[test]
fn recognizes_a_delayed_tcm_on_self() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "g4", "r5", "r4"],
            &["g3", "y4", "y1", "y5", "b3"],
        ],
        Setup {
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob plays r4", "g1");
    take_turn(&mut game, "Cathy clues red to Alice (slot 4)");
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
    assert!(game.meta[game.state.hands[Alice.ordinal()][3]].trash);
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .len(),
        0
    );
}

#[test]
fn prefers_interpreting_a_tempo_clue_over_tcm() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "g4", "r5", "r4"],
            &["g3", "y4", "y1", "y5", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["5"]);
                pre_clue(g, Alice, 4, &["4"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 1,4,5)");
    // Slots 2 and 3 should not be chop moved.
    has_status(&game, Alice, 2, CardStatus::None);
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn finesses_into_chop_moved_cards() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "g4", "r5", "r4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3,4)");
    take_turn(&mut game, "Alice discards r1 (slot 2)");
    take_turn(&mut game, "Bob clues 4 to Alice (slot 1)");
    take_turn(&mut game, "Alice plays r2 (slot 2)");
    // Slot 5 should be finessed as r3.
    has_status(&game, Alice, 5, CardStatus::Finessed);
    has_infs(&game, None, Alice, 5, &["r3"]);
}

#[test]
fn doesn_t_interpret_a_false_5cm() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r3", "r5"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 5,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slots 3,5)");
    has_status(&game, Alice, 4, CardStatus::PermissionToDiscard);
}

#[test]
fn will_not_ocm_trash() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "g4", "r1"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 3,4)");
    take_turn(&mut game, "Alice plays b1 (slot 3)");
    assert_eq!(game.last_move(), Some(Interp::Play(PlayInterp::Mistake)));
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn interprets_an_ocm_skipping_a_player() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b1", "g1", "r1", "r5"],
            &["y4", "r4", "g4", "r4", "b5"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays b1", "r1");
    has_status(&game, Alice, 4, CardStatus::ChopMoved);
}

#[test]
fn interprets_an_ocm_that_bombs() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b1", "g1", "r1", "r5"],
            &["y4", "r4", "g4", "r4", "b5"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 2, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob bombs g1", "r1");
    has_status(&game, Cathy, 5, CardStatus::ChopMoved);
}

#[test]
fn overrides_chop_focus_with_a_new_fresh_1() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r5", "b1", "g1", "r1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays g1", "y1"); // OCM once
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays y1", "p4");
    // Slot 4 is not chop moved.
    has_status(&game, Alice, 4, CardStatus::PermissionToDiscard);
}

#[test]
fn doesn_t_try_to_ocm_with_one_1_left_to_play() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            variant: TestVariant::Brown5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 3,4)");
    // Only slot 4 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}

#[test]
fn interprets_a_double_ocm() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "p4", "y4", "b4", "g4"],
            &["r4", "p4", "y4", "b4", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 1,2,3,4)");
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
    has_status(&game, Bob, 4, CardStatus::ChopMoved);
}

#[test]
fn interprets_a_double_ocm_in_2p() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "p4", "y4", "b4", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2,3,4)");
    take_turn(&mut game, "Alice plays r1 (slot 2)");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
    has_status(&game, Bob, 4, CardStatus::ChopMoved);
}

#[test]
fn interprets_a_triple_ocm_in_2p() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "p4", "y4", "b4", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2,3,4)");
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
    has_status(&game, Bob, 4, CardStatus::ChopMoved);
    has_status(&game, Bob, 3, CardStatus::ChopMoved);
}

#[test]
fn interprets_new_focus_correctly() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b3", "g3", "r3", "r5"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| {
                set_status(g, Alice, 4, CardStatus::ChopMoved);
                set_status(g, Alice, 5, CardStatus::ChopMoved);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues purple to Alice (slots 2,5)");
    has_infs(&game, None, Alice, 2, &["p1"]);
}

#[test]
fn interprets_focus_touching_only_cm_correctly() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b3", "g3", "r3", "r5"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| {
                set_status(g, Alice, 4, CardStatus::ChopMoved);
                set_status(g, Alice, 5, CardStatus::ChopMoved);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues purple to Alice (slots 4,5)");
    has_infs(&game, None, Alice, 4, &["p1"]);
}

#[test]
fn interprets_focus_touching_only_cm_and_clued_correctly() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b3", "g3", "r3", "r5"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| {
                set_status(g, Alice, 4, CardStatus::ChopMoved);
                set_status(g, Alice, 5, CardStatus::ChopMoved);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues purple to Alice (slots 2,3,4,5)");
    take_turn(&mut game, "Alice plays p1 (slot 3)");
    take_turn(&mut game, "Bob clues purple to Alice (slots 3,4,5)");
    has_infs(&game, None, Alice, 3, &["p2"]);
}

#[test]
fn prioritizes_new_cards_over_gt_eliminated_chop_moved_cards() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b1", "g1", "r5", "r2"],
            &["y2", "b2", "p3", "y1", "r4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 5, 5, 5, 5]),
            discarded: &["r3", "r4"],
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob"); // known r5 stall
    take_turn(&mut game, "Bob clues 4 to Cathy"); // r4 save
    take_turn(&mut game, "Cathy clues 1 to Alice (slot 4)"); // TCM, saving slot 5 (r3)
    take_turn(&mut game, "Alice discards b1 (slot 4)"); // draws r2 in slot 1
    take_turn(&mut game, "Bob clues red to Cathy"); // reverse finesse on r4
    let g = &game;
    has_infs(g, None, Alice, 1, &["r2"]);
    take_turn_draw(&mut game, "Cathy discards y1", "r1");
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Play {
            target: g.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays r2 (slot 1)");
    take_turn_draw(&mut game, "Bob discards r2", "p2");
    take_turn_draw(&mut game, "Cathy discards p3", "y4");
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn symmetrically_elminates_against_cm_cards() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b1", "g1", "r3", "r2"],
            &["p4", "b2", "y1", "p3", "y4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![4, 5, 5, 5, 5]),
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Cathy"); // cm trash
    take_turn(&mut game, "Cathy clues red to Alice (slots 2,3)");
    has_infs(&game, None, Alice, 2, &["r5"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn writes_an_accidental_chop_move() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards b4", "p3");
    has_status(&game, Cathy, 5, CardStatus::ChopMoved);
}
