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
fn eliminates_from_focus_direct_play() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r4", "r2", "y4", "y2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 0, 0, 4]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues purple to Alice (slots 4,5)");
    has_infs(&game, None, Alice, 5, &["p5"]);
    assert!(
        game.common
            .thinks_trash(&game, Alice.ordinal())
            .contains(&game.state.hands[Alice.ordinal()][3])
    );
}

#[test]
fn eliminates_from_focus_direct_play_2() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "i1", "g1", "i5", "i4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![5, 5, 5, 2, 1]),
            variant: TestVariant::LightPinkGray5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 2)");
    take_turn(&mut game, "Alice plays i3 (slot 2)");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 1,2)");
    has_infs(&game, None, Alice, 1, &["a2"]);
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][0])
    );
}

#[test]
fn doesn_t_count_bad_touch_when_focusing_the_last_card() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r4", "r2", "r1", "p1"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            ..Setup::default()
        },
    );
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Alice clues 1 to Bob");
    let action = ClueAction {
        giver: Alice.ordinal(),
        target: Bob.ordinal(),
        list: vec![5, 6],
        clue: BaseClue {
            kind: ClueKind::Rank,
            value: 1,
        },
    };
    let (bad_touch, trash, _) = bad_touch_result(&game, &hypo, &action);
    assert!(bad_touch.is_empty());
    assert_eq!(trash, FastBitSet::from([6]));
}

#[test]
fn doesn_t_count_bad_touch_when_focusing_a_dupe_of_the_last_card_1() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r4", "r2", "p1", "p1"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            ..Setup::default()
        },
    );
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Alice clues 1 to Bob");
    let action = ClueAction {
        giver: Alice.ordinal(),
        target: Bob.ordinal(),
        list: vec![5, 6],
        clue: BaseClue {
            kind: ClueKind::Rank,
            value: 1,
        },
    };
    let (bad_touch, trash, _) = bad_touch_result(&game, &hypo, &action);
    assert!(bad_touch.is_empty());
    assert_eq!(trash, FastBitSet::from([6]));
}

#[test]
fn doesn_t_count_bad_touch_when_focusing_a_dupe_of_the_last_card_2() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p2", "y4", "y1", "b1", "r1"],
            &["p3", "p3", "r2", "r4", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![3, 3, 3, 3, 1]),
            ..Setup::default()
        },
    );
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Alice clues 3 to Cathy");
    let action = ClueAction {
        giver: Alice.ordinal(),
        target: Cathy.ordinal(),
        list: vec![13, 14],
        clue: BaseClue {
            kind: ClueKind::Rank,
            value: 3,
        },
    };
    let (bad_touch, trash, _) = bad_touch_result(&game, &hypo, &action);
    assert!(bad_touch.is_empty());
    assert_eq!(trash, FastBitSet::from([13]));
}
