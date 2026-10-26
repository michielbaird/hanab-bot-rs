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
fn understands_muddy_save_with_red() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["r1", "m1", "m2", "m5"]);
}

#[test]
fn understands_cocoa_save_with_red() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Cocoa5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["r1", "m1", "m2", "m3", "m4", "m5"]);
}

#[test]
fn doesn_t_interpret_other_colours_as_saves() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["g1", "m1"]);
}

#[test]
fn correctly_interprets_mud_clues_in_muddy() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
            &["r1", "y4", "r3", "r3", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slots 2,3,4,5)");
    take_turn(&mut game, "Cathy clues green to Alice (slots 3,4,5)");
    has_infs(&game, None, Alice, 4, &["m1"]);
}

#[test]
fn correctly_interprets_mud_clues_in_cocoa() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
            &["r1", "y4", "r3", "r3", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slots 4,5)");
    take_turn(&mut game, "Cathy clues yellow to Alice (slots 4,5)");
    has_infs(&game, None, Alice, 5, &["m1"]);
}

#[test]
fn recognizes_normal_tempo_clues_when_the_leftmost_card_isn_t_muddy() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4"],
            &["y4", "y4", "r4", "r4"],
            &["y3", "y3", "g3", "g3"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            play_stacks: Some(vec![0, 0, 1, 0, 0]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slots 3,4)");
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 2)");
    take_turn(&mut game, "Donald clues green to Alice (slots 2,3,4)");
    has_infs(&game, None, Alice, 3, &["m1", "m2", "m3", "m4", "m5"]);
    has_infs(&game, None, Alice, 4, &["m1", "m2", "m5"]);
}

#[test]
fn wraps_around_a_mud_clue() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
            &["y4", "y4", "r3", "r3", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues yellow to Alice (slots 3,4,5)");
    take_turn(&mut game, "Cathy clues red to Alice (slots 3,4)");
    has_infs(&game, None, Alice, 3, &["m1"]);
}

#[test]
fn skips_over_known_non_muddy_cards() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4"],
            &["y4", "y4", "r4", "r4"],
            &["y3", "y3", "g3", "g3"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Muddy5,
            play_stacks: Some(vec![0, 0, 1, 0, 0]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slot 3)");
    take_turn(&mut game, "Cathy clues green to Alice (slots 1,2,4)");
    take_turn(&mut game, "Donald clues blue to Alice (slots 1,2,3)");
    has_infs(&game, None, Alice, 2, &["m1"]);
}

#[test]
fn plays_into_a_potential_muddy_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "r2", "y4", "y4"],
            &["g4", "g4", "b4", "b4"],
            &["r4", "r4", "m4", "m4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Muddy5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 3)");
    take_turn(&mut game, "Donald clues 2 to Bob");
    // If we play m1, then r2 won't connect.
    assert_eq!(
        game.me()
            .thinks_playables(&game, game.state.our_player_index, false, true),
        vec![3]
    );
}
