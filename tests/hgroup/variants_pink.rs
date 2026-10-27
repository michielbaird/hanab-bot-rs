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
fn interprets_pink_promise_on_a_play_clue() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 5)");
    // This is exactly [r3], not [r3, i1, i2, i5].
    has_infs(&game, None, Alice, 5, &["r3"]);
}

#[test]
fn gives_play_clues_with_pink_promise() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["i1", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn pink_promises_rightmost_5_in_a_5_stall() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            init: Box::new(|g| {
                set_status(g, Alice, 5, CardStatus::ChopMoved);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slots 1,2,3,5)");
    // Slot 3 is promised to be a 5.
    has_infs(&game, None, Alice, 3, &["r5", "y5", "g5", "b5", "i5"]);
}

#[test]
fn doesn_t_5_stall_on_a_pink_card() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "i2", "b4", "y4", "b3"],
        ],
        Setup {
            clue_tokens: 7,
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn understands_a_pink_trash_cm() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![1, 1, 1, 1, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slot 3)");
    for slot in [4, 5] {
        has_status(&game, Alice, slot, CardStatus::ChopMoved);
    }
    assert_eq!(
        game.common.thinks_trash(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][2]]
    );
}

#[test]
fn doesn_t_give_a_5cm_with_wrong_rank() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["i5", "b1", "r2", "i1", "g2"],
        ],
        Setup {
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            clue_tokens: 4,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn uses_pink_promise_to_eliminate_ids() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "y4", "g4", "b4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![3, 0, 0, 0, 3]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slots 4,5)"); // pink promise on slot 5
    take_turn(&mut game, "Alice discards r2 (slot 3)");
    take_turn(&mut game, "Bob clues 4 to Alice (slots 1,4,5)"); // r4 in slot 1, revealing slots 4 and 5 as pink
    take_turn(&mut game, "Alice plays r4 (slot 1)");
    take_turn_draw(&mut game, "Bob discards b4", "y3");
    // We should play slot 4 as i4.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][3]
        }
    );
}

#[test]
fn plays_with_pink_1s_assumption() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays r1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards r4", "g5");
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][4]]
    );
}

#[test]
fn fixes_a_pink_1s_assumption_without_fix_promise() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "i4", "i3", "y1", "g5"],
            &["g3", "b4", "r5", "y3", "i1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues pink to Cathy");
    take_turn_draw(&mut game, "Bob plays y1", "b4");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
    take_turn(&mut game, "Alice clues 5 to Bob");
    has_infs(&game, None, Bob, 4, &["i2", "i3", "i4", "i5"]);
}

#[test]
fn fixes_a_pink_1s_assumption_with_fix_promise() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "i4", "i3", "y1", "g4"],
            &["g3", "b4", "r5", "y3", "i1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice clues pink to Cathy");
    take_turn_draw(&mut game, "Bob plays y1", "b4");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)");
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 3
        }
    );
    let mut fix_hypo = game.clone();
    take_turn(&mut fix_hypo, "Alice clues 3 to Bob");
    assert_eq!(fix_hypo.last_move(), Some(Interp::Clue(ClueInterp::Fix)));
    has_infs(&fix_hypo, None, Bob, 4, &["i3"]);
    let mut bad_hypo = game.clone();
    take_turn(&mut bad_hypo, "Alice clues 4 to Bob");
    assert_eq!(
        bad_hypo.last_move(),
        Some(Interp::Clue(ClueInterp::Mistake))
    );
}

#[test]
fn doesn_t_perform_ocms_in_pink() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 4,5)");
    // Alice must play slot 5 (not allowed to OCM by playing slot 4).
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn understands_finessed_1s_in_pink() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "y4", "i4", "r4", "r4"],
            &["r2", "g4", "g4", "b4", "b4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 1,5)");
    // Both slots 1 and 5 are playable, since slot 1 is known r1..
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![4, 0]
    );
}

#[test]
fn plays_1s_in_order_even_after_a_certain_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "g2", "r3", "i4"],
            &["g1", "g3", "i1", "i1"],
            &["r4", "r4", "b3", "r5"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 1,2,3,4)");
    take_turn(&mut game, "Donald clues red to Bob");
    take_turn(&mut game, "Alice clues green to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "b1");
    take_turn_draw(&mut game, "Cathy discards g1", "y2");
    // Only slot 4 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}

#[test]
fn doesn_t_play_filled_in_pink_cards_as_1s() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays r1 (slot 5)");
    take_turn(&mut game, "Bob clues pink to Alice (slots 1,5)");
    // This is a pink fix.
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}

#[test]
fn doesn_t_play_filled_in_omni_cards_as_1s() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "b4", "r4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Omni5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Alice plays r1 (slot 5)");
    take_turn(&mut game, "Bob clues red to Alice (slots 1,4)");
    take_turn(&mut game, "Alice plays y1 (slot 5)");
    take_turn(&mut game, "Bob clues green to Alice (slots 1,5)");
    // This is an omni fix (only r1 is playable).
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][1]]
    );
}

#[test]
fn prompts_leftmost_when_rank_matches_in_a_non_pink_suit() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "i3", "i1", "r4", "g5"],
            &["g4", "y3", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Cathy clues red to Bob");
    // Alice should prompt slot 3 as r2.
    has_infs(&game, None, Alice, 3, &["r2"]);
}

#[test]
fn prompts_leftmost_when_rank_matches_in_a_pink_suit() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "i3", "i4", "r4", "g5"],
            &["g4", "y3", "r3", "b3", "b3"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 3 as i2.
    has_infs(&game, None, Alice, 3, &["i2"]);
}

#[test]
fn finesses_when_rank_mismatches() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "r1", "r4", "g5"],
            &["g4", "i3", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Alice discards g4 (slot 2)");
    take_turn(&mut game, "Bob clues pink to Cathy");
    // Alice should finesse slot 1 as i2.
    has_infs(&game, None, Alice, 1, &["i2"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn prompts_with_two_rank_mismatches() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "i2", "r4", "r4", "g5"],
            &["g4", "y3", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Cathy clues 5 to Alice (slots 2,3)"); // Alice: [xx ?5 i? ?2 ?2]
    take_turn(&mut game, "Alice discards b3 (slot 1)");
    take_turn(&mut game, "Bob clues blue to Cathy");
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 3 as i1.
    has_infs(&game, None, Alice, 3, &["i1"]);
}

#[test]
fn prompts_even_with_rank_mismatch_if_no_finesse_position() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["i3", "b3", "r1", "r4", "g5"],
            &["g4", "y3", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            discarded: &["y4"],
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues yellow to Alice (slots 3,4,5)");
    take_turn(&mut game, "Cathy clues 5 to Alice (slots 1,2)");
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "r1");
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 1 as i2.
    has_infs(&game, None, Alice, 1, &["i2"]);
}

#[test]
fn understands_a_mismatched_rank_prompt() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "i3", "r1", "r4", "g5"],
            &["i2", "b5", "y2", "y3", "y4"],
        ],
        Setup {
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            discarded: &["y4"],
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues yellow to Cathy");
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn(&mut game, "Cathy clues red to Alice (slot 3)");
    take_turn(&mut game, "Alice plays r1 (slot 3)");
    take_turn(&mut game, "Bob clues 3 to Alice (slots 2,3)");
    // Alice should understand Cathy's i2 is prompted, and not self-finesse.
    has_status(&game, Alice, 1, CardStatus::None);
    has_infs(&game, None, Cathy, 1, &["i2"]);
}

#[test]
fn prompts_the_card_with_the_most_positive_info() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["i3", "r4", "y4", "g4", "b4"],
            &["r3", "y3", "g3", "b3", "i3"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2", "3"]);
                pre_clue(g, Alice, 4, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 5 as i2.
    has_status(&game, Alice, 1, CardStatus::None);
    has_infs(&game, None, Alice, 5, &["i2"]);
}

#[test]
fn prompts_leftmost_when_it_must_be_pink_from_good_touch() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["i4", "r4", "y4", "g4", "b4"],
            &["r3", "y3", "g3", "b3", "i3"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2", "3"]);
                pre_clue(g, Alice, 4, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Bob");
    // Alice should prompt slot 4 as i3.
    has_status(&game, Alice, 1, CardStatus::None);
    has_infs(&game, None, Alice, 4, &["i3"]);
}

#[test]
fn gives_a_pink_choice_tempo_clue() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y3", "r2", "b3", "b1"],
            &["r1", "i1", "i3", "i2", "g5"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues pink to Cathy");
    take_turn_draw(&mut game, "Cathy plays i1", "b2");
    take_turn(&mut game, "Alice clues 4 to Cathy");
    has_infs(&game, None, Cathy, 4, &["i2"]);
}

#[test]
fn receives_a_pink_choice_tempo_clue() {
    let mut game = setup(
        hgroup(6),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "y3", "r2", "b3", "b1"],
            &["r3", "b1", "b3", "b2", "g5"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Alice (slots 3,4,5)");
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn(&mut game, "Bob clues 4 to Alice (slots 3,4,5)");
    has_infs(&game, None, Alice, 4, &["i1"]);
    has_status(&game, Alice, 2, CardStatus::None); // Not TCCM
}

#[test]
fn interprets_a_pink_choice_tempo_clue_over_a_pink_fix() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "r1", "r4", "g5"],
            &["g4", "i3", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Alice (slots 1,5)");
    take_turn(&mut game, "Alice plays i2 (slot 5)");
    take_turn(&mut game, "Bob clues 2 to Alice (slot 2)");
    // Slot 2 should be i3, not trash.
    has_infs(&game, None, Alice, 2, &["i3"]);
}

#[test]
fn doesn_t_self_prompt_a_pink_positional() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "r1", "i4", "i3"],
            &["i2", "g4", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            discarded: &["i3", "i4"],
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Bob"); // i3, i4 save
    take_turn(&mut game, "Alice clues 4 to Bob");
    // Not a valid self-prompt, even if Cathy finesses.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_self_prompt_with_a_rewinded_pink_positional() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o3", "r3", "y4", "g4", "b4"],
            &["o4", "r4", "y4", "g4", "b4"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Omni5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 4, &["1", "5"]);
                pre_clue(g, Alice, 5, &["1", "5"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Alice (slots 4,5)"); // positional o2
    take_turn(&mut game, "Alice clues yellow to Cathy"); // selfish finesse
    take_turn_draw(&mut game, "Bob discards b4", "r1");
    take_turn(&mut game, "Cathy clues red to Alice (slots 1,4,5)"); // reverse f
    take_turn(&mut game, "Alice plays o2 (slot 4)");
    take_turn_draw(&mut game, "Bob plays o3", "y1");
    take_turn_draw(&mut game, "Cathy plays o4", "g1");
    let g = &game;
    // Bob hasn't demonstrated a red finesse yet.
    has_infs(g, None, Alice, 2, &["r1", "r2"]);
    take_turn(&mut game, "Alice bombs o2 (slot 5)"); // good touch bomb
    take_turn_draw(&mut game, "Bob plays r1", "b1");
    // Bob demonstrated the finesse.
    has_infs(&game, None, Alice, 3, &["r2"]);
}

#[test]
fn plays_into_complex_pink_tempo_clues() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y4", "r4", "g1"],
            &["i1", "g4", "g4", "r4", "y4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::PinkLPink6,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3,4)");
    take_turn(&mut game, "Cathy clues green to Bob");
    take_turn(&mut game, "Alice plays l1 (slot 4)");
    take_turn_draw(&mut game, "Bob plays g1", "y3");
    take_turn(&mut game, "Cathy clues 2 to Alice (slots 1,3,4)");
    let g = &game;
    // Pink fix promise
    has_infs(g, None, Alice, 4, &["i2"]);
    take_turn(&mut game, "Alice clues pink to Cathy");
    take_turn(&mut game, "Bob clues 3 to Alice (slots 1,3,4)"); // Pink choice tempo on slot 3
    take_turn_draw(&mut game, "Cathy plays i1", "r3");
    let g = &game;
    // Slot 4 as [i2,l2] must be played first.
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![g.state.hands[Alice.ordinal()][3]]
    );
    take_turn(&mut game, "Alice plays i2 (slot 4)");
    take_turn_draw(&mut game, "Bob discards r4", "g3");
    take_turn(&mut game, "Cathy clues 2 to Alice (slots 2,4)"); // Pink choice tempo on slot 2
    // Slot 4 as [i3, l2] must be played first.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][3]]
    );
}

#[test]
fn gives_a_pink_trash_fix() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "i3", "i1", "r5", "g4"],
            &["g4", "y3", "r3", "i1", "i2"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues pink to Cathy");
    take_turn_draw(&mut game, "Cathy plays i2", "b3");
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Rank {
            target: Cathy.ordinal(),
            value: 1
        }
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");
    assert_eq!(
        game.common.thinks_trash(&game, Cathy.ordinal()),
        vec![game.state.hands[Cathy.ordinal()][4]]
    );
}

#[test]
fn interprets_a_pink_trash_fix() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "r1", "r5", "g4"],
            &["g4", "i3", "r2", "b3", "b1"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Pink5,
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues pink to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays i2 (slot 5)");
    take_turn(&mut game, "Bob clues 1 to Alice (slot 5)");
    assert_eq!(
        game.common.thinks_trash(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][4]]
    );
}

#[test]
fn doesn_t_give_a_finesse_that_looks_like_a_fix() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "y4", "i1", "b1", "r1"],
        ],
        Setup {
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "b3");
    take_turn(&mut game, "Alice clues 3 to Bob");
    // This is a pink fix, not a self-finesse.
    assert_eq!(
        game.common
            .thinks_playables(&game, Bob.ordinal(), false, true),
        vec![game.state.hands[Bob.ordinal()][4]]
    );
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_interpret_a_pink_trash_fix_after_a_prompt() {
    let mut game = setup(
        hgroup(3),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y3", "r4", "y4", "i2"],
            &["g4", "i4", "g3", "b3", "b4"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::Pink5,
            clue_tokens: 7,
            init: Box::new(|g| {
                fully_known(g, Alice, 1, "b1");
                pre_clue(g, Bob, 5, &["pink"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slots 4,5)");
    take_turn(&mut game, "Cathy clues 2 to Bob"); // prompt Alice's slot 4
    let g = &game;
    has_infs(g, None, Alice, 4, &["i1"]);
    take_turn(&mut game, "Alice plays b1 (slot 1)"); // Alice draws a new 5
    take_turn(&mut game, "Bob clues 5 to Alice (slots 1,4,5)"); // 5 Stall (not fix)
    has_infs(&game, None, Alice, 4, &["i1"]);
}
