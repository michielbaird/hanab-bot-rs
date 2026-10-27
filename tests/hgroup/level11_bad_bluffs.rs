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
fn doesn_t_give_a_self_colour_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y2", "g2", "p2", "b2"],
            &["p4", "r3", "g1", "y4", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");
    take_turn(&mut game, "Alice clues blue to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_a_card_that_isn_t_immediately_playable() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "b5", "y1", "r1"],
            &["y4", "p1", "g3", "g4"],
            &["y3", "g1", "g3", "r1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 1, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Donald");
    take_turn_draw(&mut game, "Donald plays r1", "p1");
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 3 to Donald");
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues green to Donald");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_through_a_self_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "b5", "y1", "r4", "y4"],
            &["p2", "p3", "g3", "g4", "y5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 3 to Cathy");
    // This is an illegal bluff target.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_when_cathy_will_connect_1() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "b5", "y1", "r4", "y4"],
            &["p2", "r2", "b2", "g4", "y5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_when_it_can_t_be_recognized_by_all_players() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "b5", "y2", "y2", "y4"],
            &["p2", "r2", "b2", "g4", "y5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_when_cathy_will_connect_2() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "r1", "p2", "r4"],
            &["p3", "p2", "g1", "g4"],
            &["r1", "r1", "r4", "g3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues purple to Bob");
    take_turn_draw(&mut game, "Donald plays r1 (slot 1)", "r5");
    let g = &game;
    has_infs(g, None, Bob, 3, &["p2"]);
    take_turn(&mut game, "Alice clues 3 to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_on_top_of_unknown_queued_cards() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "r2", "y1", "r4"],
            &["p2", "b2", "r3", "y4"],
            &["g1", "g2", "g3", "p4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues red to Cathy");
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 2 to Cathy");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues blue to Cathy");
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad3 = game.clone();
    take_turn(&mut bad3, "Alice clues purple to Cathy");
    assert_eq!(bad3.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_a_previously_finessed_player() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "y1", "g1", "b5", "r4"],
            &["g2", "r2", "r3", "y5", "y4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob"); // self-finesse
    take_turn(&mut game, "Alice clues 2 to Cathy"); // stacking g1 on top
    take_turn_draw(&mut game, "Bob plays y1", "i1");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 3 to Cathy");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues red to Cathy");
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_a_previously_finessed_player_2() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "y1", "g1", "b5", "r4"],
            &["g2", "r2", "r3", "y5", "y4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob"); // self-finesse
    take_turn(&mut game, "Alice clues 2 to Cathy"); // stacking g1 on top
    take_turn_draw(&mut game, "Bob plays y1", "i1");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 3 to Cathy");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues red to Cathy");
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_on_top_of_a_possibly_layered_gd() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "y1", "r2", "y1", "r4"],
            &["g1", "b2", "g3", "g5", "p4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Cathy");
    take_turn_draw(&mut game, "Cathy discards g1", "y4");
    // With g1 already queued, we can't bluff Bob's y1.
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 2 to Cathy");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues blue to Cathy");
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_when_bob_will_colour_prompt() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "y1", "y1", "y5"],
            &["p4", "b2", "r3", "b5", "y4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "g1");
    take_turn(&mut game, "Cathy clues 5 to Bob");
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 3 to Cathy");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues red to Cathy");
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_when_bob_will_rank_prompt() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "r2", "y1", "y1", "y5"],
            &["g4", "b2", "p3", "b5", "y4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 1, 0, 0, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn_draw(&mut game, "Bob plays y2", "g1");
    take_turn(&mut game, "Cathy clues 5 to Bob");
    let mut bad1 = game.clone();
    take_turn(&mut bad1, "Alice clues 3 to Cathy");
    assert_eq!(bad1.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
    let mut bad2 = game.clone();
    take_turn(&mut bad2, "Alice clues purple to Cathy");
    assert_eq!(bad2.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_give_a_delayed_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "g3", "p4", "p3", "b3"],
            &["g1", "y5", "g4", "r1", "b1"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Cathy");
    take_turn_draw(&mut game, "Cathy plays b1", "r4");
    take_turn(&mut game, "Alice clues 3 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_use_an_illegal_bluff_target() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "g1", "p2", "y3"],
            &["r1", "b1", "r3", "b4"],
            &["r1", "p3", "r3", "r2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 3 to Donald");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_give_a_self_bluff_that_looks_like_a_reverse_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "g2", "r3", "y3", "p3"],
            &["r3", "r4", "y5", "p4", "r2"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 1, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 4 to Bob");
    // Bob will wait for r3 to play, causing the bluff to fail.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_through_a_wrong_prompt() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "b1", "r3", "y3"],
            &["b1", "g4", "b4", "b3"],
            &["g2", "p3", "r3", "b5"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 0, 0, 2, 0]),
            discarded: &["b4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Cathy"); // [b3,b4]
    take_turn(&mut game, "Alice clues blue to Donald");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_symmetrically_finessed_cards_1() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "y3", "p4", "y4"],
            &["p3", "r4", "b2", "b3"],
            &["r3", "y2", "r1", "b5"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 0, 0, 2, 0]),
            discarded: &["b4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Cathy"); // could be r3 (Bob f) or b3 (direct)
    take_turn(&mut game, "Alice clues yellow to Donald");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_symmetrically_finessed_cards_2() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "b3", "p4", "y4"],
            &["y1", "r4", "b2", "b3"],
            &["r3", "p2", "r1", "b5"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 0, 0, 2, 0]),
            discarded: &["b4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Bob"); // could be y3 (Cathy + Alice f) or b3 (direct)
    take_turn(&mut game, "Alice clues purple to Donald");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_bluff_when_a_delayed_self_interpretation_exists() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "p4", "g1", "p3", "y4"],
            &["r4", "y1", "b2", "r1", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 2, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");
    take_turn(&mut game, "Alice clues 4 to Bob");
    // Bob will always play r2 first, so the bluff doesn't work.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
