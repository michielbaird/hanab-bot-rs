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
fn gives_a_5_stall_before_ending_early_game() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r5", "b4", "y4", "b3"],
        ],
        Setup {
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
    assert_eq!(game.hg().stall_interp, Some(StallInterp::Stall5));
}

#[test]
fn doesn_t_5_stall_on_a_trash_5() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r5", "b4", "y4", "b3"],
        ],
        Setup {
            discarded: &["r4", "r4"],
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
