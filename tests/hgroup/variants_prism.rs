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
fn understands_prism_save_with_colour() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Prism5,
            discarded: &["i3"],
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["g1", "i3"]);
}

#[test]
fn understands_prism_saves_aren_t_all_ranks() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Prism5,
            discarded: &["i3"],
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["b1"]);
}
