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
fn understands_k3_4_save_with_black() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Black5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues black to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["k1", "k3", "k4"]);
}

#[test]
fn doesn_t_save_k2_with_black() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "k2"],
        ],
        Setup {
            variant: TestVariant::Black5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues black to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_save_k5_with_black() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "k5"],
        ],
        Setup {
            variant: TestVariant::Black5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues black to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn relaxes_save_restrictions_when_multiple_cards_are_newly_clued() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Black5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues black to Alice (slots 4,5)");
    has_infs(&game, None, Alice, 5, &["k1", "k2", "k3", "k4", "k5"]);
}

#[test]
fn doesn_t_relax_save_restrictions_when_only_1_card_is_newly_clued() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Black5,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["black"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues black to Alice (slots 4,5)");
    has_infs(&game, None, Alice, 4, &["k1", "k3", "k4"]);
}

#[test]
fn relaxes_save_restrictions_when_filling_in() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Black5,
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["5"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues black to Alice (slots 4,5)");
    has_infs(&game, None, Alice, 4, &["k1", "k2", "k3", "k4"]);
}

#[test]
fn relaxes_save_restrictions_when_avoiding_bad_touch() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b4", "b4", "r2", "k2"],
            &["y3", "y3", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Black5,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues black to Bob");
    has_status(&game, Alice, 1, CardStatus::None);
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Save)));
}

#[test]
fn doesn_t_relax_save_restrictions_when_no_bad_touch() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b4", "b4", "r3", "k2"],
            &["y3", "y3", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Black5,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues black to Bob");
    has_status(&game, Alice, 1, CardStatus::Finessed);
}
