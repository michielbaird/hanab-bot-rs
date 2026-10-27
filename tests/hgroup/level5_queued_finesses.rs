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
fn understands_a_queued_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "y4", "b4", "b4"],
            &["y3", "y3", "p4", "p4"],
            &["g2", "y4", "b3", "b3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Donald");
    let g = &game;
    has_infs(g, None, Alice, 1, &["g1"]);
    has_status(g, Alice, 1, CardStatus::Finessed);
    take_turn(&mut game, "Cathy clues red to Bob");
    take_turn(&mut game, "Donald clues 5 to Alice (slot 4)");
    let g = &game;
    // Only Alice's slot 1 is playable.
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![g.state.hands[Alice.ordinal()][0]]
    );
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    // Alice's slot 2 should be r1.
    has_infs(&game, None, Alice, 2, &["r1"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn understands_a_delayed_queued_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "g3", "g4", "r5", "b4"],
            &["r3", "b3", "r4", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob"); // g1, g2 on us
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    take_turn(&mut game, "Bob clues red to Cathy");
    // Only Alice's slot 2 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][1]]
    );
}

#[test]
fn waits_for_a_queued_finesse_to_resolve() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4"],
            &["g2", "p4", "b4", "b4"],
            &["g1", "r1", "p4", "y5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues green to Cathy");
    take_turn(&mut game, "Bob clues red to Alice (slot 2)"); // r2 finesse
    has_infs(&game, None, Alice, 2, &["r1", "r2"]);
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn_draw(&mut game, "Donald plays g1", "b1");
    // Alice should wait for Donald.
    has_infs(&game, None, Alice, 2, &["r1", "r2"]);
}

#[test]
fn plays_queued_finesses_in_the_right_order() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "r5", "b4"],
            &["g2", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");
    take_turn(&mut game, "Alice plays b1 (slot 1)"); // expecting r1 finesse
    take_turn(&mut game, "Bob clues green to Cathy"); // g2 reverse finesse
    take_turn_draw(&mut game, "Cathy discards p3", "y1");
    // Only Alice's slot 2 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][1]]
    );
}

#[test]
fn waits_for_an_older_unplayable_finesse_before_playing_into_a_new_one() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "b3", "g4", "p5"],
            &["r1", "r2", "r3", "y3"],
            &["b4", "r5", "p3", "y2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Donald"); // r5 finesse, Alice has r4
    take_turn_draw(&mut game, "Cathy plays r1", "b1");
    take_turn(&mut game, "Donald clues 5 to Alice (slot 4)");
    take_turn(&mut game, "Alice discards y4 (slot 3)"); // r4 now in slot 2
    take_turn(&mut game, "Bob clues yellow to Donald"); // y2 finesse, Alice has y1
    // Alice cannot play y1 in slot 1, because y1 could be layered in the r4 finesse.
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}

#[test]
fn doesn_t_wait_for_an_older_finesse_when_it_can_t_be_layered() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "b3", "g4", "p4"],
            &["r1", "r2", "r3", "y3"],
            &["b4", "r5", "p3", "y2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Donald"); // r5 finesse, Alice has r4
    take_turn_draw(&mut game, "Cathy plays r1", "b1");
    take_turn(&mut game, "Donald clues 5 to Alice (slots 3,4)");
    take_turn(&mut game, "Alice discards y4 (slot 2)"); // r4 now in slot 2
    take_turn(&mut game, "Bob clues yellow to Donald"); // y2 finesse, Alice has y1
    // Alice can play y1 in slot 1, because the queued r4 can't be layered.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
    has_infs(&game, None, Alice, 1, &["y1"]);
}

#[test]
fn orders_complex_finesses_correctly() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b5", "r4", "y2", "p4"],
            &["p2", "r1", "b3", "g4"],
            &["r3", "b1", "r4", "b1"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 1)"); // g2 self-finesse
    take_turn(&mut game, "Cathy clues 2 to Bob"); // + y1 to Alice's finesses
    take_turn(&mut game, "Donald clues green to Cathy"); // + g3 to Alice's finesses
    take_turn(&mut game, "Alice plays g1 (slot 2)");
    let g = &game;
    has_status(g, Alice, 3, CardStatus::Finessed);
    // hasStatus(g, Alice, 4, CardStatus.Finessed)
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays r1", "g1");
    take_turn(&mut game, "Donald clues 5 to Bob");
    let g = &game;
    // Alice should play y1 in slot 3.
    has_infs(g, None, Alice, 3, &["y1"]);
    take_turn(&mut game, "Alice plays y1 (slot 3)");
    has_infs(&game, None, Alice, 4, &["g3"]);
}
