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
