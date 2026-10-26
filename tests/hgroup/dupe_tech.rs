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
fn understands_a_simple_duplicitous_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "r1"],
            &["r1", "y1", "b4", "b4", "g4"],
        ],
        Setup {
            starting: Bob,
            init: Box::new(|g| {
                fully_known(g, Bob, 5, "r1");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues yellow to Alice (slot 5)");
    take_turn_draw(&mut game, "Cathy plays r1", "g4");
    has_infs(&game, None, Alice, 5, &["y2"]);
}

#[test]
fn receives_a_simple_duplicitous_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p4", "b4", "b4", "y2"],
            &["r4", "r4", "y4", "y4", "r1"],
        ],
        Setup {
            starting: Cathy,
            init: Box::new(|g| {
                fully_known(g, Cathy, 5, "r1");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob");
    let g = &game;
    has_status(g, Alice, 1, CardStatus::Finessed);
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn understands_a_duplicitous_layered_finesse_where_the_giver_s_card_is_unknown() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "r4", "r4", "r1"],
            &["r1", "y1", "y4", "y4", "g4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn(&mut game, "Bob clues yellow to Alice (slot 5)");
    take_turn_draw(&mut game, "Cathy plays r1", "g4");
    has_infs(&game, None, Alice, 5, &["y2"]);
}

#[test]
fn doesn_t_save_a_potentially_duplicated_2() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "r4", "r4", "r2"],
            &["g4", "g4", "y4", "y4", "p4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 4,5)");
    take_turn(&mut game, "Cathy clues blue to Bob");
    assert!(matches!(game.take_action(), PerformAction::Discard { .. }));
}

#[test]
fn doesn_t_save_a_potentially_duplicated_1() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "r4", "r4", "r1"],
            &["g4", "g4", "y4", "y4", "p4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 4,5)");
    take_turn(&mut game, "Cathy clues blue to Bob");
    assert_eq!(game.common.hypo_stacks, vec![0, 0, 0, 1, 0]);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}
