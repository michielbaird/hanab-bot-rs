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
fn discards_an_orange_playable() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues orange to Alice (slot 1)");
    has_infs(&game, None, Alice, 1, &["o1"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn discards_an_orange_finesse() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o2", "y4", "y4", "o4", "r4"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues orange to Bob");
    has_infs(&game, None, Alice, 1, &["o1"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn discards_an_orange_double_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o3", "y4", "y4", "o4", "r4"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues orange to Bob");
    take_turn(&mut game, "Alice plays o1 (slot 1)");
    has_infs(&game, None, Alice, 2, &["o2"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn discards_an_orange_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y4", "b4", "o4", "r4"],
            &["g4", "y4", "b4", "o4", "r4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Orange5,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["orange"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 5)");
    has_infs(&game, None, Alice, 1, &["o1"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn discards_an_orange_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o2", "y4", "y4", "o4", "r4"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues orange to Bob");
    take_turn(&mut game, "Alice discards r1 (slot 1)");
    // Alice is still promised for o1.
    has_infs(&game, None, Alice, 2, &["o1"]);
}

#[test]
fn lets_a_playable_orange_card_discard() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "o2"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues orange to Alice (slot 1)");
    has_infs(&game, None, Alice, 1, &["o1"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn assumes_a_play_target_is_not_orange_when_possible() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "o1", "r1"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            variant: TestVariant::Orange5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues orange to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slot 1)");
    has_infs(&game, None, Alice, 1, &["r2"]);
}

#[test]
fn understands_an_orange_fix_clue_on_chop() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues orange to Alice (slot 5)");
    has_infs(&game, None, Alice, 5, &["o3", "o4"]);
}

#[test]
fn gives_an_orange_fix_clue_on_chop() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "o4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: 4
        }
    );
}

#[test]
fn gives_an_orange_fix_clue_on_a_rank_playable_card() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "o1", "r1"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "y4");
    // Alice should fix with orange so Bob doesn't discard the o1.
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: 4
        }
    );
}

#[test]
fn understands_an_orange_fix_clue_with_played_orange_cards() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 0, 0, 4]),
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues orange to Alice (slot 5)");
    assert_eq!(
        game.common.thinks_trash(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][4]]
    );
}

#[test]
fn gives_an_orange_fix_clue_with_played_orange_cards() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "o1"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 0, 0, 4]),
            variant: TestVariant::Orange5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: 4
        }
    );
}

#[test]
fn understands_an_orange_gentleman_s_discard_is_to_chop() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o1", "g4", "g4", "b4", "b4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues orange to Bob");
    take_turn_draw(&mut game, "Bob discards o1", "r4");
    has_infs(&game, None, Alice, 5, &["o1"]);
    assert_eq!(game.take_action(), PerformAction::Discard { target: 0 });
}

#[test]
fn understands_an_orange_layered_gentleman_s_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o1", "g4", "g4", "b4", "b4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues orange to Bob");
    take_turn_draw(&mut game, "Bob discards o1", "r4");
    take_turn(&mut game, "Alice discards r1 (slot 5)");
    // o1 is still promised in Alice's new chop.
    has_infs(&game, None, Alice, 5, &["o1"]);
    // Bob is not shouted for.
    has_status(&game, Bob, 5, CardStatus::PermissionToDiscard);
}

#[test]
fn understands_an_orange_baton_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o2", "g4", "g4", "b4", "b4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Orange5,
            clue_tokens: 4,
            init: Box::new(|g| {
                fully_known(g, Bob, 1, "o2");
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards o2", "o1");
    // o2 is promised on Alice's chop.
    has_infs(&game, None, Alice, 5, &["o2"]);
    assert_eq!(game.chop(Alice.ordinal()), Some(1));
    take_turn(&mut game, "Alice clues orange to Bob");
    take_turn_draw(&mut game, "Bob plays o1", "r4");
    // Alice should drag the o2 to the discard stacks.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn screams_for_an_unplayable_orange_on_chop() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o3", "g4", "g4", "r1", "o3"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            clue_tokens: 1,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "r5");
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice discards b4 (slot 4)");
    assert_eq!(
        game.last_move(),
        Some(Interp::Discard(DiscardInterp::Emergency))
    );
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn discarding_chop_ends_early_game_even_if_it_plays() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "o1"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Bob,
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    let g = &game;
    assert!(g.hg().in_early_game);
    take_turn_draw(&mut game, "Bob plays o1", "r5");
    assert!(!game.hg().in_early_game);
}

#[test]
fn discarding_chop_ends_early_game_even_if_it_bombs() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "o3"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Bob,
            clue_tokens: 2,
            ..Setup::default()
        },
    );
    let g = &game;
    assert!(g.hg().in_early_game);
    take_turn_draw(&mut game, "Bob bombs o3", "r5");
    assert!(!game.hg().in_early_game);
}

#[test]
fn doesn_t_try_to_insert_an_orange_card_into_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "o2", "g4", "b4"],
            &["r4", "y4", "g4", "b4"],
            &["r3", "y3", "g3", "b3"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Cathy,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    let g = &game;
    has_infs(g, None, Alice, 1, &["r1"]);
    take_turn(&mut game, "Donald clues orange to Bob");
    has_infs(&game, None, Alice, 1, &["r1"]);
    has_infs(&game, None, Alice, 2, &["o1"]);
    // Still, only slot 1 is playable, because it might be layered.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn doesn_t_allow_an_orange_card_to_be_a_layer() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o1", "r1", "g4", "b4", "o4"],
            &["r4", "y4", "g4", "b4", "o4"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slot 1)");
    // Slot 1 must be r1.
    has_infs(&game, None, Alice, 1, &["r1"]);
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn doesn_t_try_to_insert_a_layer_into_an_orange_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "o2", "g4", "b4"],
            &["r4", "y4", "g4", "b4"],
            &["r3", "y3", "g3", "b3"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Cathy,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues orange to Bob");
    let g = &game;
    has_infs(g, None, Alice, 1, &["o1"]);
    take_turn(&mut game, "Donald clues red to Bob");
    has_infs(&game, None, Alice, 1, &["o1"]);
    has_infs(&game, None, Alice, 2, &["r1"]);
    // Still, only slot 1 is playable, because it might be layered.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn discards_an_orange_card_from_a_pos_misplay() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "y1", "g1", "b1", "o1"],
            &["r1", "y1", "g1", "b1", "o1"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Cathy,
            play_stacks: Some(vec![4, 5, 5, 5, 4]),
            clue_tokens: 0,
            discarded: &[
                "r2", "r3", "r4", "y2", "y3", "y4", "g2", "g3", "g4", "b2", "b3",
            ],
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 1);
    take_turn_draw(&mut game, "Cathy bombs r1", "b4");
    // Alice should discard slot 1 as o5.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn recognizes_an_orange_misplay_as_a_pos_dc() {
    let mut game = setup(
        hgroup(8),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "y1", "g1", "b1", "o1"],
            &["o1", "y1", "g1", "b1", "r1"],
        ],
        Setup {
            variant: TestVariant::Orange5,
            starting: Cathy,
            play_stacks: Some(vec![4, 5, 5, 5, 4]),
            clue_tokens: 0,
            discarded: &[
                "r2", "r3", "r4", "y2", "y3", "y4", "g2", "g3", "g4", "b2", "b3",
            ],
            ..Setup::default()
        },
    );
    let g = &game;
    assert_eq!(g.state.cards_left, 1);
    take_turn_draw(&mut game, "Cathy bombs o1", "b4");
    // Alice doesn't have o5 in slot 1.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}
