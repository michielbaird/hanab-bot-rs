#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn prefers_to_give_direct_ranks() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "g1", "b1", "g3", "g4"],
        ],
        Setup { ..Setup::default() },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 1
        }
    );
}

#[test]
fn understands_direct_ranks_are_not_referential() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b5", "y4", "g2", "r4", "y3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");
    has_status(&game, Alice, 4, CardStatus::None);
}

#[test]
fn eliminates_direct_ranks_from_focus() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b5", "y4", "g2", "r4", "y3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 0, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");
    has_status(&game, Alice, 4, CardStatus::None);
    has_infs(&game, None, Alice, 2, &["g1"]);
    // Alice's slot 3 should be trash
    let trash = game.common.thinks_trash(&game, Alice.ordinal());
    assert!(trash.contains(&game.state.hands[Alice.ordinal()][2]));
}

#[test]
fn understands_playable_fill_ins_are_not_referential() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b5", "y4", "g2", "r4", "y3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slots 2,3)");
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    // Bob reveals r2 as a safe action.
    take_turn(&mut game, "Bob clues 2 to Alice (slots 1,3)");
    has_status(&game, Alice, 4, CardStatus::None);
}

#[test]
fn doesn_t_give_unloaded_clues_that_connect_through_own_hand() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "y5", "g5", "r2", "p3"],
        ],
        Setup {
            init: Box::new(|g| {
                fully_known(g, Alice, 2, "r1");
            }),
            ..Setup::default()
        },
    );
    // Alice should not give purple.
    assert!(!matches!(
        game.take_action(),
        PerformAction::Colour {
            target: 1,
            value: 4
        }
    ));
}

#[test]
fn it_interprets_a_gd() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "r2", "g1", "g2", "p2"],
            &["r3", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            init: Box::new(|g| {
                fully_known(g, Bob, 1, "y1");
            }),
            starting: Bob,
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards y1", "y4");
    has_status(&game, Alice, 5, CardStatus::GentlemansDiscard);
    has_infs(&game, None, Alice, 5, &["y1"]);
    assert!(
        game.common
            .obvious_playables(&game, Alice.ordinal())
            .contains(&0)
    );
}

#[test]
fn it_interprets_a_layered_gd() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "r2", "g1", "g2", "p2"],
            &["y1", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            init: Box::new(|g| {
                fully_known(g, Cathy, 1, "y1");
            }),
            starting: Cathy,
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards y1", "y4");
    take_turn(&mut game, "Alice plays b1 (slot 5)");
    // The card originally in slot 4 has moved to slot 5.
    has_status(&game, Alice, 5, CardStatus::GentlemansDiscard);
    has_infs(&game, None, Alice, 5, &["y1"]);
    assert!(
        game.common
            .obvious_playables(&game, Alice.ordinal())
            .contains(&1)
    );
}

#[test]
fn it_counts_dupes_as_bad_touch() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r4", "g3", "r1", "r4"],
            &["y3", "y3", "g4", "g4", "b3"],
        ],
        Setup {
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["4"]);
            }),
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Alice clues red to Bob");
    let Some(scala_bot::basics::Action::Clue(clue)) =
        hypo.state.action_list.last().unwrap().first().cloned()
    else {
        panic!("expected a clue");
    };
    let (bad_touch, trash, _) =
        scala_bot::basics::clue_result::bad_touch_result(&game, &hypo, &clue);
    assert_eq!(bad_touch, scala_bot::collections::FastBitSet::from([8, 6]));
    assert!(trash.is_empty());
}
