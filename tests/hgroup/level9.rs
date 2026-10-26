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
fn doesn_t_dda_stall_before_level_9() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "g4", "b4", "b3", "g3"],
            &["y4", "y4", "r4", "r3", "p3"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards p3", "g2");
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_dda_stall_in_2p() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "g4", "b4", "b3", "g3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards g3", "p3");
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn understands_a_locked_hand_stall() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "g4", "b4", "b3"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["p4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues purple to Alice (slot 4)");
    has_infs(&game, None, Alice, 4, &["p1", "p2", "p3", "p4", "p5"]);
}

#[test]
fn understands_a_fill_in_stall_on_cm_d_cards() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "g4", "b4", "b3"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slot 2)");
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues purple to Alice (slot 3)");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
    has_infs(&game, None, Alice, 3, &["p2", "p3", "p4", "p5"]);
}

#[test]
fn understands_a_save_can_look_like_a_stall() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "g4", "b4", "b3"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["b3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues 3 to Bob");
    has_infs(&game, None, Bob, 4, &["r3", "y3", "g3", "b3", "p3"]);
}

#[test]
fn doesn_t_interpret_a_stall_when_a_play_clue_is_available() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "g4", "b4", "b3"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues purple to Alice (slot 4)");
    // Can't be LHS, since b1 is available.
    has_infs(&game, None, Alice, 4, &["p1"]);
}

#[test]
fn doesn_t_interpret_a_stall_when_a_save_clue_is_available() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y3", "g4", "b3", "b4"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["b4", "p4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues purple to Alice (slot 4)");
    // Can't be LHS, since b4 is available.
    has_infs(&game, None, Alice, 4, &["p1", "p4"]);
}

#[test]
fn doesn_t_interpret_a_stall_when_loaded() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "r2", "b4", "g4"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Donald");
    take_turn(&mut game, "Cathy clues red to Donald");
    take_turn(&mut game, "Donald clues green to Alice (slot 4)");
    // Can't be LHS, since Donald can play r5.
    has_infs(&game, None, Alice, 4, &["g1"]);
}

#[test]
fn interprets_a_finesse_when_a_play_clue_is_available() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "g4", "b4", "g2"],
            &["y4", "y4", "r4", "r3"],
            &["y5", "r5", "b5", "g5"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues green to Bob (slot 4)");
    // Can't be LHS, since b1 is available.
    has_infs(&game, None, Alice, 1, &["g1"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn interprets_a_finesse_when_a_fill_in_is_available() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "g4", "g2", "b4"],
            &["y4", "y4", "r5", "r4"],
            &["r2", "y2", "b5", "g5"],
        ],
        Setup {
            play_stacks: Some(vec![2, 0, 0, 0, 2]),
            discarded: &["r4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Donald");
    take_turn(&mut game, "Bob clues red to Cathy"); // r4 save
    take_turn(&mut game, "Cathy clues 2 to Donald");
    take_turn(&mut game, "Donald clues red to Cathy");
    // Can't be a Hard Burn, since filling r5 is available.
    has_infs(&game, None, Alice, 1, &["r3"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_a_tempo_clue_when_there_are_better_clues_available() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "r1", "g2", "b4", "p4"],
            &["y4", "y4", "r3", "y2", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            discarded: &["p4"],
            clue_tokens: 6,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 3, &["red"]);
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards g4", "p2"); // g4 dda
    take_turn(&mut game, "Alice clues red to Cathy"); // p4 save to Bob, y2 save to Cathy
    // Cathy's red card should be known r3, and y2 should be chop moved from TCCM.
    has_infs(&game, None, Cathy, 4, &["r3"]);
    has_status(&game, Cathy, 5, CardStatus::ChopMoved);
}

#[test]
fn understands_a_tempo_clue_stall() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "r1", "g2", "b4", "g3"],
            &["y4", "y4", "r3", "p4", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            clue_tokens: 6,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 3, &["red"]);
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards g4", "p4");
    take_turn(&mut game, "Alice clues 3 to Cathy"); // nothing to clue
    // Cathy's red card should be known r3, and p4 shouldn't be chop moved.
    has_infs(&game, None, Cathy, 4, &["r3"]);
    has_status(&game, Cathy, 5, CardStatus::None);
}

#[test]
fn finds_all_clues_in_a_locked_hand_situation() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "r4", "b2", "b2", "r1"],
            &["y4", "y4", "r4", "r3", "g2"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy");
    take_turn(&mut game, "Bob clues 5 to Alice (slots 1,2,3,4,5)");
    take_turn(&mut game, "Cathy clues red to Bob");
    let mut fill_in1 = game.clone();
    take_turn(&mut fill_in1, "Alice clues 3 to Bob");
    assert_eq!(fill_in1.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
    assert_eq!(fill_in1.hg().stall_interp, Some(StallInterp::FillIn));
    let mut fill_in2 = game.clone();
    take_turn(&mut fill_in2, "Alice clues 4 to Bob");
    assert_eq!(fill_in2.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
    assert_eq!(fill_in2.hg().stall_interp, Some(StallInterp::FillIn));
    let mut lhs = game.clone();
    take_turn(&mut lhs, "Alice clues 3 to Cathy");
    assert_eq!(lhs.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
    assert_eq!(lhs.hg().stall_interp, Some(StallInterp::Locked));
    // 2 to Cathy is not a valid Hard Burn.
    let mut illegal = game.clone();
    take_turn(&mut illegal, "Alice clues 2 to Cathy");
    assert_eq!(illegal.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn gives_a_bad_touch_save_in_stalling_situations() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "b3", "r2", "p2"],
            &["y5", "y4", "r4", "g2", "g4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 2, 2, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn_draw(&mut game, "Cathy discards g4", "p4");
    // Alice is in DDA, she should clue 2 to Bob even though it bad touches.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 2
        }
    );
}

#[test]
fn t_5_stalls_to_the_one_closest_to_chop() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "y4", "r5", "g2", "r3"],
            &["y5", "r4", "b3", "b4", "b1"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    // Alice should 5 Stall on Cathy, since Bob's 5 is farther away from chop.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Cathy.ordinal(),
            value: 5
        }
    );
}

#[test]
fn respects_fpe_for_5_stalls() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "r5", "g2", "r3"],
            &["y5", "r4", "b3", "b4", "b1"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 3)");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
}

#[test]
fn doesn_t_use_fpe_to_end_early_game() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    assert!(game.early_game_clue(Alice.ordinal()).is_some());
}

#[test]
fn considers_an_fpe() {
    let game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g5", "b4", "p3"],
            &["r1", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            clue_tokens: 5,
            ..Setup::default()
        },
    );
    // 5 Stalling saves p3 for 1 round.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
}

#[test]
fn doesn_t_consider_fpe_when_there_is_a_save_clue() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g5", "b4", "p3"],
            &["r1", "y4", "g4", "b4", "p5"],
        ],
        Setup {
            clue_tokens: 5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn allows_a_5_stall_fpe_outside_of_early_game() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y5", "r5", "b5", "g5"],
            &["y4", "p5", "r4", "r3"],
            &["b1", "g4", "b4", "b3"],
        ],
        Setup {
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    let mut fpe = game.clone();
    take_turn(&mut fpe, "Bob clues 5 to Cathy");
    assert_eq!(fpe.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
    // But doesn't allow a locked hand stall
    let mut finesse = game.clone();
    take_turn(&mut finesse, "Bob clues red to Cathy");
    assert_eq!(finesse.last_move(), Some(Interp::Clue(ClueInterp::Play)));
}

#[test]
fn gives_a_play_clue_in_a_stalling_situation_even_when_it_looks_like_a_save() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "g3", "b3", "m4"],
            &["r4", "y4", "g4", "m4", "b3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 0, 0, 2, 0]),
            discarded: &["m3"],
            variant: TestVariant::Rainbow5,
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slots 1,2,3,4,5)");
    // Alice must clue Cathy's b3, even though it gets 0 playables.
    assert!(matches!(
        game.take_action(),
        PerformAction::Rank { target, value: 3 } | PerformAction::Colour { target, value: 3 } if target == Cathy.ordinal()
    ));
    // Alice is not allowed to give a Fill-In Stall.
    let mut mistake_game = game.clone();
    take_turn(&mut mistake_game, "Alice clues 4 to Bob");
    assert_eq!(
        mistake_game.last_move(),
        Some(Interp::Clue(ClueInterp::Mistake))
    );
    let mut stall_game = game.clone();
    take_turn(&mut stall_game, "Alice clues 3 to Cathy");
    assert_eq!(
        stall_game.last_move(),
        Some(Interp::Clue(ClueInterp::PlayLooksSave))
    );
}

#[test]
fn will_give_a_5_stall_in_dda() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "y5", "b2", "g4"],
            &["b3", "p4", "b4", "b2"],
            &["y4", "b4", "r4", "r3"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 0,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Donald discards r3", "p3");
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Stall)));
}

#[test]
fn will_discard_in_dda_if_the_copy_is_visible() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "y5", "b2", "g4"],
            &["b3", "p4", "b4", "b2"],
            &["y4", "b4", "r4", "r3"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 0,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Donald discards r3", "p3");
    let g = &game;
    // Since Alice can see the other copy of r3, she should discard.
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    // Alice can't stall when not in DDA.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_trigger_dda_from_a_sarcastic_discard() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "y5", "b2", "g4"],
            &["b3", "b1", "g1", "b3"],
            &["b1", "b4", "r4", "r3"],
        ],
        Setup {
            starting: Bob,
            discarded: &["b1"],
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Cathy");
    take_turn(&mut game, "Cathy clues blue to Donald");
    take_turn_draw(&mut game, "Donald discards b1", "p3");
    assert_eq!(game.hg().dda, None);
}

#[test]
fn doesn_t_try_to_finesse_a_player_who_assumes_dda() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "g4", "b4", "p4"],
            &["r2", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 0,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 1, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards p4", "p3");
    take_turn(&mut game, "Alice clues red to Cathy");
    // Bob will think this is a fill-in stall, so it won't work.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn finesses_a_player_who_knows_it_s_not_dda() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "g4", "b4", "b4"],
            &["r2", "y4", "g4", "p4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 0,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 1, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards p4 (slot 5)", "p3");
    take_turn(&mut game, "Alice clues red to Cathy");
    // Bob knows we aren't in DDA, so he will react.
    has_status(&game, Bob, 1, CardStatus::Finessed);
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
}

#[test]
fn interprets_an_asymmetric_5cm() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "b4"],
            &["r4", "y4", "g4", "p4", "p4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards b4 (slot 5)", "p3");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 4)");
    // Because Cathy is not in DDA, we should discard slot 3.
    has_status(&game, Alice, 5, CardStatus::ChopMoved);
}

#[test]
fn doesn_t_give_a_bad_asymmetric_5cm() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "p4", "g4", "b5", "b4"],
            &["r4", "y4", "g4", "y4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards p4 (slot 5)", "p3");
    take_turn(&mut game, "Alice clues 5 to Bob");
    // Bob doesn't know that we aren't in DDA, so this 5cm doesn't work.
    has_status(&game, Bob, 5, CardStatus::PermissionToDiscard);
}

#[test]
fn plays_into_anxiety() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y5", "y2", "b4", "g4"],
            &["b1", "g4", "b4", "b2"],
            &["y4", "y4", "r4", "r3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slots 2,3,4)");
    take_turn(&mut game, "Donald clues 2 to Alice (slot 1)");
    // Alice should play slot 2 as r5.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn doesn_t_assume_anxiety_if_there_are_clues_available() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y5", "y2", "b4", "g4"],
            &["b1", "g4", "b4", "b2"],
            &["y4", "y4", "r4", "r3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 5 to Alice (slots 1,2,3,4)");
    // Alice should clue.
    assert!(matches!(
        game.take_action(),
        PerformAction::Rank { .. } | PerformAction::Colour { .. }
    ));
}

#[test]
fn doesn_t_play_into_impossible_anxiety() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r5", "y2", "b4", "g4"],
            &["b1", "g4", "b4", "b2"],
            &["y4", "y4", "r4", "r3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 5 to Alice (slots 1,2,3,4)");
    // Alice should discard, since it isn't possible to play any card.
    assert!(matches!(game.take_action(), PerformAction::Discard { .. }));
}

#[test]
fn forces_the_next_player_into_anxiety_by_playing_a_connecting_card() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r5", "y5", "b5", "g5"],
            &["b3", "g4", "b4", "b4"],
            &["y4", "y4", "r4", "r3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Bob");
    take_turn(&mut game, "Donald clues red to Alice (slot 1)");
    // Alice should play slot 1.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn forces_the_next_player_into_anxiety_by_playing_an_unrelated_card() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r5", "y5", "b5", "g5"],
            &["b3", "g4", "b4", "b4"],
            &["y4", "y4", "r4", "r3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Bob");
    take_turn(&mut game, "Donald clues blue to Alice (slot 1)");
    // Alice should play slot 1.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn gives_an_anxiety_clue_to_the_next_player() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "y5", "b5", "g5"],
            &["r1", "g4", "b4", "b4"],
            &["y4", "y4", "p4", "p3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            discarded: &["r3", "r4", "b3"],
            clue_tokens: 3,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Bob");
    take_turn(&mut game, "Donald clues green to Alice (slot 1)");
    // Alice should clue red/3 to Bob as anxiety
    let action = game.take_action();
    let clues = [
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 3,
        },
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: Colour::Red.ordinal(),
        },
    ];
    assert!(clues.contains(&action), "Expected clue, got {action:?}");
}
