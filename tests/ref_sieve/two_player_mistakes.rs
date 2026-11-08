#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::interp::{ClueInterp, Interp};
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn resets_ptd_after_an_unexpected_slot_1_discard() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_status(&game, Bob, 1, CardStatus::PermissionToDiscard);
    take_turn_draw(&mut game, "Bob bombs p4", "b3");
    take_turn(&mut game, "Alice discards y3 (slot 1)");
    has_status(&game, Bob, 1, CardStatus::None);
    has_status(&game, Bob, 2, CardStatus::PermissionToDiscard);
    take_turn_draw(&mut game, "Bob discards b3", "p3");
    // We should reset PTD on Bob's slot 2.
    has_status(&game, Bob, 2, CardStatus::None);
}

#[test]
fn an_unknown_play_that_can_only_be_a_clued_dupe_is_not_valued_as_nan() {
    // Every 1 that Alice's clued card could be is clued in Bob's hand, so there is nothing it could be that is worth playing.
    let game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "b1", "p1"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["1"]);
                for slot in 1..=5 {
                    pre_clue(g, Bob, slot, &["1"]);
                }
            }),
            ..Setup::default()
        },
    );
    let order = game.state.hands[Alice.ordinal()][0];
    let value = hanab_bot::ref_sieve::state_eval::eval_action_impl(
        &game,
        &hanab_bot::basics::action::Action::Play(hanab_bot::basics::action::PlayAction {
            player_index: Alice.ordinal(),
            order,
            id: None,
        }),
    );
    assert_eq!(value, -100.0);
}
