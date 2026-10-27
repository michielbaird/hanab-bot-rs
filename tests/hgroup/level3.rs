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
fn orders_starting_hand_1s_from_right_to_left() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y5", "r5", "b1", "p3", "y4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 3,4)");
    assert_eq!(ordered_1s(&game, Alice), vec![1, 2]);
}

#[test]
fn orders_fresh_1s() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y5", "r5", "b1", "p3", "y4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice bombs b4 (slot 1)");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,4)");
    assert_eq!(ordered_1s(&game, Alice), vec![10, 1]);
}

#[test]
fn orders_chop_focus() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y5", "r5", "b1", "p3", "y4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice bombs b4 (slot 1)");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2,5)");
    assert_eq!(ordered_1s(&game, Alice), vec![0, 10, 3]);
}

#[test]
fn doesn_t_prompt_playable_1s() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "r2", "g3", "r5", "b3"],
            &["r4", "b4", "g4", "y3", "p4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");
    take_turn(&mut game, "Cathy clues red to Bob");
    // Alice's 1 can still be any 1 (not prompted to be r1).
    has_infs(&game, None, Alice, 2, &["r1", "y1", "g1", "b1", "p1"]);
}

#[test]
fn recognizes_the_correct_focus_of_a_1_clue() {
    let game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "r2", "g3", "r5", "b3"],
            &["r4", "b1", "g4", "y1", "p4"],
        ],
        Setup { ..Setup::default() },
    );
    let clue = BaseClue {
        kind: ClueKind::Rank,
        value: 1,
    };
    let action = ClueAction {
        giver: Alice.ordinal(),
        target: Cathy.ordinal(),
        list: game
            .state
            .clue_touched(&game.state.hands[Cathy.ordinal()], &clue),
        clue,
    };
    let focus_result = game.determine_focus(&game, &action);
    // The focus of the clue is Cathy's slot 4.
    assert_eq!(focus_result.focus, game.state.hands[Cathy.ordinal()][3]);
}

#[test]
fn orders_1s_correctly_when_cluing_chop_moved_cards() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "b1", "r4", "y4", "g1"],
        ],
        Setup {
            init: Box::new(|g| {
                set_status(g, Bob, 5, CardStatus::ChopMoved);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(ordered_1s(&game, Bob), vec![8, 9, 5]);
}

#[test]
fn orders_1s_correctly_when_only_cluing_chop_moved_cards() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "b4", "r4", "r1", "y1"],
        ],
        Setup {
            init: Box::new(|g| {
                set_status(g, Bob, 4, CardStatus::ChopMoved);
                set_status(g, Bob, 5, CardStatus::ChopMoved);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    assert_eq!(ordered_1s(&game, Bob), vec![6, 5]);
}

#[test]
fn understands_a_sarcastic_discard() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "b4", "g4", "y3", "y1"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slot 4)");
    take_turn(&mut game, "Alice clues yellow to Bob");
    take_turn_draw(&mut game, "Bob discards y1", "r1");
    has_infs(&game, None, Alice, 4, &["y1"]);
}

#[test]
fn sarcastic_discards_without_assuming_position() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "g2", "b2", "r4", "p2"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 0, 1, 0]),
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn(&mut game, "Bob clues blue to Alice (slot 1)");
    take_turn(&mut game, "Alice discards b2 (slot 1)");
    let b2 = game.state.expand_short("b2").unwrap();
    assert!([1, 2, 4].iter().all(|&i| {
        let order = game.state.hands[Bob.ordinal()][i];
        game.players[Bob.ordinal()].thoughts[order]
            .inferred
            .contains(b2)
    }));
}

#[test]
fn preserves_info_lock_after_a_sarcastic_discard() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "g2", "b2", "r4", "p2"],
            &["y3", "y1", "b5", "g3", "r2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 1, 1, 2, 2]),
            clue_tokens: 6,
            init: Box::new(|g| {
                fully_known(g, Cathy, 5, "r2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 1,2)");
    take_turn_draw(&mut game, "Cathy discards r2", "p5");
    has_infs(&game, None, Alice, 1, &["y2", "g2"]);
    has_infs(&game, None, Alice, 2, &["r2"]);
}

#[test]
fn generates_a_link_from_a_sarcastic_discard() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y3", "g2", "b2", "r4", "b5"],
            &["y4", "y1", "p2", "g3", "r2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            clue_tokens: 6,
            init: Box::new(|g| {
                fully_known(g, Cathy, 5, "r2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 3,4,5)");
    take_turn_draw(&mut game, "Cathy discards r2", "g1");
    let r2 = game.state.expand_short("r2").unwrap();
    assert!(game.common.links.iter().any(|l| matches!(l,
        Link::Sarcastic { orders, id } if orders.contains(1) && orders.contains(2) && *id == r2)));
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues green to Alice (slot 3)");
    has_infs(&game, None, Alice, 4, &["r2"]);
}

#[test]
fn plays_into_a_prompt_after_a_sarcastic_discard() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b5", "b4", "r4", "r1"],
            &["y4", "y3", "p2", "g3", "g2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 1, 0, 0]),
            clue_tokens: 6,
            init: Box::new(|g| {
                fully_known(g, Cathy, 5, "g2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 4,5)");
    take_turn_draw(&mut game, "Cathy discards g2", "g1"); // Alice has g2 among her two clued 2s
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "g3");
    take_turn(&mut game, "Cathy clues green to Bob");
    has_infs(&game, None, Alice, 4, &["g2"]);
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}

#[test]
fn discards_chop_when_a_sarcastic_discard_can_t_be_done() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r2", "b4", "r4", "r4"],
            &["g4", "y4", "b4", "p4", "p4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 2, 2, 2, 2]),
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 5)");
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][3]
        }
    );
}

#[test]
fn correctly_connects_to_a_play_clue_after_a_fix() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "b4", "g4", "r4", "p4"],
            &["y2", "b4", "g4", "r4", "p4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Alice plays r1 (slot 5)");
    take_turn(&mut game, "Bob clues yellow to Cathy"); // y2 play, promising Alice y1
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 2)"); // Alice: xx x2 xx x1 x1
    let g = &game;
    has_infs(g, None, Alice, 2, &["r2", "g2", "b2", "p2"]);
    take_turn(&mut game, "Alice plays y1 (slot 5)");
    take_turn(&mut game, "Bob clues yellow to Alice (slot 5)"); // Fix on dupe y1
    // Alice's 2 must have connected to r1.
    has_infs(&game, None, Alice, 3, &["r2"]);
}

#[test]
fn interprets_a_fix_with_no_new_info() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "b4", "g4", "r4", "p4"],
            &["y4", "b4", "g4", "r4", "p4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            clue_tokens: 4,
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,3,4)");
    take_turn(&mut game, "Alice plays p1 (slot 4)");
    take_turn_draw(&mut game, "Bob discards p4", "y3");
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 3,4)");
    // The no-info clue fixes slot 4, while slot 3 remains playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][2]]
    );
    assert_eq!(
        game.common.thinks_trash(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}
