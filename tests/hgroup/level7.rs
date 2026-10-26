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
fn performs_a_sdcm() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "b4", "r5"],
            &["g1", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slot 5)");
    let g = &game;
    // Alice should discard slot 4 as a SDCM.
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice discards y4 (slot 4)");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn doesn_t_sdcm_for_non_critical_non_playable() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "b4", "g3"],
            &["g1", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slot 5)");
    // Alice should play slot 5.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn will_sdcm_for_a_delayed_playable() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "b4", "r3"],
            &["g1", "b3", "y4", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slot 5)");
    // Alice should play slot 5.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][3]
        }
    );
}

#[test]
fn doesn_t_sdcm_when_target_is_loaded() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "r3", "g4", "b4", "r5"],
            &["r4", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 5)");
    take_turn(&mut game, "Cathy clues green to Bob");
    // Alice should play slot 5.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn stalls_after_a_scream() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r2", "g4", "b4", "b3"],
            &["g1", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 1,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Cathy");
    take_turn_draw(&mut game, "Cathy discards p3", "p4");
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
    // Alice should 5 stall on Bob.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
}

#[test]
fn screams_at_1_clue_when_the_next_player_will_become_locked() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y5", "g5", "b5", "r5"],
            &["g3", "b3", "r3", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 3,
            discarded: &["r4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 1)");
    take_turn(&mut game, "Cathy clues 5 to Bob");
    let g = &game;
    // Alice should discard slot 5 as a SDCM.
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][4]
        }
    );
    take_turn(&mut game, "Alice discards y3 (slot 5)");
    has_status(&game, Bob, 1, CardStatus::ChopMoved);
}

#[test]
fn doesn_t_mistake_a_sdcm_for_a_gen_discard() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "p1", "p1"],
            &["g1", "b3", "r2", "y3", "r4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Cathy");
    take_turn_draw(&mut game, "Cathy discards r4", "y3");
    // Our slot 5 should be chop moved, since p1 isn't a valid gen target.
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
}

#[test]
fn performs_a_shout_discard() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "p1", "g5"],
            &["g1", "b3", "r3", "y3", "p3"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 2,
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 4,5)");
    let g = &game;
    // Alice should discard slot 4 as a Shout Discard.
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice discards y1 (slot 4)");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn stalls_after_a_shout_discard() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r4", "g4", "b4", "b3"],
            &["p1", "g1", "r4", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 1,
            play_stacks: Some(vec![1, 1, 0, 1, 1]),
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Cathy");
    take_turn_draw(&mut game, "Cathy discards p1", "p4");
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
    // Alice should 5 Stall on Bob.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
}

#[test]
fn doesn_t_interpret_a_shout_discard_when_a_finesse_is_disproved() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r4", "g4", "b4", "b3"],
            &["p1", "g1", "r4", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 6,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 1)");
    take_turn_draw(&mut game, "Cathy discards p3", "p4");
    // Alice is not chop moved.
    has_status(&game, Alice, 5, CardStatus::None);
}

#[test]
fn interprets_a_shout_when_discarding_a_dupe() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 3,4)");
    take_turn(&mut game, "Cathy clues red to Alice (slots 3,4)");
    take_turn(&mut game, "Alice discards r2 (slot 4)");
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn performs_a_gen_discard() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "b4", "p2"],
            &["g1", "b3", "r2", "y3", "r5"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slot 5)");
    let g = &game;
    // Alice should discard slot 4 as a Generation Discard.
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice discards b4 (slot 4)");
    // Bob's slot 5 should not be chop moved.
    has_status(&game, Bob, 5, CardStatus::None);
}

#[test]
fn interprets_gen_over_sdcm() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "b4", "r1"],
            &["g1", "b3", "r2", "y3", "p5"],
        ],
        Setup {
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob discards b4", "g3");
    // Could be scream on Cathy or generation on Alice: assume scream for now
    assert_eq!(game.hg().dc_status, DcStatus::Scream);
    has_status(&game, Cathy, 5, CardStatus::ChopMoved);
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    // Alice now knows it was a Generation Discard.
    has_status(&game, Cathy, 5, CardStatus::None);
}

#[test]
fn doesn_t_gen_if_they_can_connect() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r2", "g4", "b4", "r1"],
            &["r3", "b3", "r5", "y3", "b1"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 2,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays r2 (slot 5)");
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays r3", "p4");
    // Alice should play slot 5 (r4 -> r5) rather than generating for Cathy.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn doesn_t_gen_if_next_player_can_connect() {
    let mut game = setup(
        hgroup(7),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "r2", "i1", "g5"],
            &["r3", "b3", "r5", "i2"],
            &["r3", "y4", "b5", "g4"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 1,
            play_stacks: Some(vec![0, 0, 1, 0, 0]),
            variant: TestVariant::Prism5,
            init: Box::new(|g| {
                fully_known(g, Alice, 4, "g2");
                fully_known(g, Bob, 3, "i1");
            }),
            ..Setup::default()
        },
    );
    // Getting i2, but could be y1 or y2.
    take_turn(&mut game, "Donald clues yellow to Cathy");
    // Alice should play slot 4 (g2) instead of generating for Cathy.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][3]
        }
    );
}
