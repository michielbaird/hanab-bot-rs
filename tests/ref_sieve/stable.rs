#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::interp::{ClueInterp, Interp};
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn it_understands_a_ref_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "r3", "g5"],
            &["p4", "b5", "p2", "b1", "g4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues green to Bob");
    has_status(&game, Bob, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Bob, 1, &["r1", "y1", "b1", "p1"]);
}

#[test]
fn it_understands_a_gapped_ref_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "b1", "p2", "b5", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues purple to Bob");
    has_status(&game, Bob, 2, CardStatus::DragToPlay);
    has_infs(&game, None, Bob, 2, &["r1", "y1", "g1", "b1"]);
}

#[test]
fn it_understands_a_chop_ref_play() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b2", "p2", "b5", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    has_infs(&game, None, Bob, 1, &["b1"]);
}

#[test]
fn it_understands_a_loaded_colour_clue() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b2", "p2", "b5", "g4"],
            &["b1", "g2", "b2", "r3", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn_draw(&mut game, "Cathy plays b1", "y3");
    has_infs(&game, None, Cathy, 3, &["b2"]);
}

#[test]
fn it_understands_a_playable_colour_clue_is_also_referential() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b2", "p2", "b5", "g4"],
            &["b1", "g2", "b2", "r3", "g5"],
        ],
        Setup {
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 2)");
    has_status(&game, Alice, 1, CardStatus::DragToPlay);
}

#[test]
fn it_understands_a_ref_discard() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "b5", "g3"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 4 to Bob");
    has_status(&game, Bob, 2, CardStatus::CalledToDiscard);
}

#[test]
fn it_gives_a_ref_discard() {
    let game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "b5", "g3"],
            &["b3", "g2", "r2", "r3", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    // Alice should clue 4 to Bob.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 4
        }
    );
}

#[test]
fn it_understands_a_lock() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p5", "p2", "p2", "b4", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup {
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 4 to Bob");
    assert!(game.common.thinks_locked(&game, Bob.ordinal()));
}

#[test]
fn it_doesn_t_focus_the_wrong_card_for_the_last_id() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "b1", "p1"],
            &["r1", "y1", "g1", "b1", "p1"],
        ],
        Setup {
            play_stacks: Some(vec![3, 3, 3, 3, 2]),
            starting: Cathy,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["purple"]);
            }),
            ..Setup::default()
        },
    );
    // Although Alice could play slot 2, she should play slot 5 first.
    take_turn(&mut game, "Cathy clues 3 to Alice (slots 2,5)");
    assert_eq!(game.take_action(), PerformAction::Play { target: 0 });
}

#[test]
fn understands_a_trash_push_with_rank() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "b1", "r2", "r4", "g5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 1, 1, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slot 3)");
    let slot2 = &game.meta[game.state.hands[Alice.ordinal()][1]];
    let playables = game
        .common
        .thinks_playables(&game, Alice.ordinal(), false, true);
    assert_eq!(slot2.status, CardStatus::DragToPlay);
    assert!(playables.contains(&slot2.order));
}

#[test]
fn understands_a_trash_push_touching_old_cards() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "b1", "r2", "r4", "g5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 2, 2, 2, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 2,3)");
    take_turn(&mut game, "Alice plays p2 (slot 2)");
    take_turn(&mut game, "Bob clues 2 to Alice (slots 1,3)");
    let slot5 = &game.meta[game.state.hands[Alice.ordinal()][4]];
    let playables = game
        .common
        .thinks_playables(&game, Alice.ordinal(), false, true);
    assert_eq!(slot5.status, CardStatus::DragToPlay);
    assert!(playables.contains(&slot5.order));
}

#[test]
fn wraps_around_a_loaded_trash_push() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "b1", "r2", "r4", "g5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 2, 2, 2, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slots 2,3)");
    take_turn(&mut game, "Alice plays p2 (slot 2)");
    take_turn(&mut game, "Bob clues 1 to Alice (slot 1)");
    let slot5 = &game.meta[game.state.hands[Alice.ordinal()][4]];
    let playables = game
        .common
        .thinks_playables(&game, Alice.ordinal(), false, true);
    assert_eq!(slot5.status, CardStatus::DragToPlay);
    assert!(playables.contains(&slot5.order));
}
