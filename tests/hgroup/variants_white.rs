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
fn understands_loaded_3_play_clues() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::White5,
            discarded: &["w3"],
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 4)");
    // Since Alice is loaded, this finesses slot 1.
    has_status(&game, Alice, 1, CardStatus::Finessed);
}
