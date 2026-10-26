//! Port of `test/variants.scala` (run against the neutral convention; see `empathy.rs`).

mod common;

use common::*;
use scala_bot::basics::clue::{ClueKind, ClueLike};

#[test]
fn understands_prism_touch() {
    let mut game = setup(
        neutral(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "b1", "r2", "r3", "g5"],
        ],
        Setup {
            starting: Player::Bob,
            variant: TestVariant::Prism5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 1)");
    has_poss(
        &game,
        None,
        Player::Alice,
        1,
        &["r1", "r2", "r3", "r4", "r5", "i1", "i5"],
    );
    take_turn(&mut game, "Alice clues blue to Bob");
    take_turn(&mut game, "Bob clues green to Alice (slot 2)");

    has_poss(
        &game,
        None,
        Player::Alice,
        2,
        &["g1", "g2", "g3", "g4", "g5", "i3"],
    );
}

#[test]
fn doesnt_clue_prism() {
    let game = setup(
        neutral(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "b1", "r2", "r3", "g5"],
        ],
        Setup {
            starting: Player::Bob,
            variant: TestVariant::Prism5,
            ..Setup::default()
        },
    );

    assert!(
        !game
            .state
            .all_valid_clues(Player::Bob.ordinal())
            .iter()
            .any(|c| c.is_eq_to(ClueKind::Colour, 4))
    );
}
