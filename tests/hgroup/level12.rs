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
fn understands_focus_inversion() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "b3", "p3"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slot 5)");
    take_turn(&mut game, "Alice plays r1 (slot 5)");
    take_turn(&mut game, "Bob clues yellow to Alice (slots 1,5)");
    has_infs(&game, None, Alice, 1, &["y1"]);
    has_infs(&game, None, Alice, 5, &["y2", "y3", "y4", "y5"]);
}

#[test]
fn interprets_a_tcm_from_a_stale_1() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "b3", "p3"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards p3", "r5");
    take_turn(&mut game, "Cathy clues 1 to Alice (slot 4)");
    // This is a Trash Chop Move.
    assert!(game.meta[game.state.hands[Alice.ordinal()][3]].trash);
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
    assert_eq!(
        game.common.thinks_trash(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
}

#[test]
fn doesn_t_randomly_skip_a_1() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "b3", "p3"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 3,4)");
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![
            game.state.hands[Alice.ordinal()][2],
            game.state.hands[Alice.ordinal()][3]
        ]
    );
    assert!(game.common.thinks_trash(&game, Alice.ordinal()).is_empty());
}

#[test]
fn skips_a_stale_1() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "b3", "p3"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards p3", "r5");
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,3,4)");
    let g = &game;
    // This focuses slot 3, and slot 4 is kt.
    assert!(g.meta[g.state.hands[Alice.ordinal()][3]].trash);
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![
            g.state.hands[Alice.ordinal()][1],
            g.state.hands[Alice.ordinal()][2]
        ]
    );
    assert_eq!(
        g.common.thinks_trash(g, Alice.ordinal()),
        vec![g.state.hands[Alice.ordinal()][3]]
    );
    has_infs(g, None, Alice, 2, &["b1", "p1"]);
    take_turn(&mut game, "Alice plays b1 (slot 3)");
    // This is not an OCM on Bob.
    has_status(&game, Bob, 5, CardStatus::PermissionToDiscard);
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

#[test]
fn gives_a_stale_1_s_chop_move() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "r1", "p3"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards p4", "b4");
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 1
        }
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
    assert!(game.meta[game.state.hands[Bob.ordinal()][3]].trash);
}

#[test]
fn understands_stale_1_s_in_pink() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "b3", "i3"],
            &["r4", "y4", "g4", "b4", "i4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 4,
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards i3", "r5");
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,3,4)");
    let g = &game;
    // This focuses slot 3, and slot 4 is some pink.
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![g.state.hands[Alice.ordinal()][2]]
    );
    assert!(g.common.thinks_trash(g, Alice.ordinal()).is_empty());
    take_turn(&mut game, "Alice plays b1 (slot 3)");
    // This is not an OCM on Bob.
    has_status(&game, Bob, 5, CardStatus::PermissionToDiscard);
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][2]]
    );
    assert!(game.common.thinks_trash(&game, Alice.ordinal()).is_empty());
}

#[test]
fn understands_blockers_in_stale_1_s() {
    let mut game = setup(
        hgroup(12),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "r1", "p3"],
            &["r4", "y4", "g4", "g1", "p4"],
        ],
        Setup {
            starting: Alice,
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice discards y1 (slot 5)");
    take_turn(&mut game, "Bob clues 1 to Cathy");
    let g = &game;
    // This is a TCM, because Cathy's 1 was not blocked.
    has_status(g, Cathy, 5, CardStatus::ChopMoved);
    take_turn(&mut game, "Cathy clues 1 to Alice (slot 4)");
    // Alice's y1 was blocking, so this is a play clue.
    has_status(&game, Alice, 5, CardStatus::None);
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}
