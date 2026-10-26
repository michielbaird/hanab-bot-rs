//! Port of `test/empathy.scala`.
//!
//! Tests that used Reactor or H-Group in Scala run here against the neutral test convention,
//! since those conventions aren't ported yet; their assertions are about empathy only.

mod common;

use common::*;
use scala_bot::basics::clue::ClueKind;

#[test]
#[should_panic(expected = "copies")]
fn it_fails_impossible_setup() {
    setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r1", "r1", "r1"],
        ],
        Setup::default(),
    );
}

#[test]
fn it_elims_from_count() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r1", "r2", "r2"],
        ],
        Setup {
            starting: Player::Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 5)");

    has_poss(
        &game,
        Some(Player::Alice),
        Player::Alice,
        5,
        &["r3", "r4", "r5"],
    );
}

#[test]
fn it_marks_trash_correctly_when_losing_all_copies_of_a_card() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "r1", "b5", "b3"],
        ],
        Setup {
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice discards b3 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b3", "b1");

    for short in ["b3", "b4", "b5"] {
        assert!(
            game.state
                .is_basic_trash(game.state.expand_short(short).unwrap()),
            "{short}"
        );
    }
    assert!(
        !game
            .state
            .is_critical(game.state.expand_short("b5").unwrap())
    );
}

#[test]
fn it_visibly_elims_5s() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx"], // p5, t5 in slots 3 and 4
            &["g2", "b1", "r1", "g5"],
            &["g3", "p1", "b3", "b5"],
            &["r3", "b2", "r1", "y5"],
        ],
        Setup {
            starting: Player::Donald,
            play_stacks: Some(vec![5, 0, 0, 0, 0, 0]),
            variant: TestVariant::NoVar6,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues green to Alice (slot 1)");
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues 5 to Alice (slots 3,4)");

    has_poss(&game, None, Player::Alice, 3, &["p5", "t5"]);
    has_poss(&game, None, Player::Alice, 4, &["p5", "t5"]);
    has_poss(&game, None, Player::Bob, 4, &["g5"]);
    has_poss(&game, None, Player::Cathy, 4, &["b5"]);
    has_poss(&game, None, Player::Donald, 4, &["y5"]);
}

#[test]
fn it_visibly_elims_mixed_cards() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "b1", "r4", "r5", "y3"],
            &["y5", "p1", "b3", "b5", "g3"],
        ],
        Setup {
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            discarded: &["r1", "r1", "r2", "r3"],
            init: Box::new(|g| {
                pre_clue(g, Player::Alice, 5, &["red"]);
                pre_clue(g, Player::Bob, 3, &["red"]);
                pre_clue(g, Player::Bob, 4, &["red"]);
            }),
            ..Setup::default()
        },
    );

    // Everyone knows that Alice's card is known r4.
    has_poss(&game, None, Player::Alice, 5, &["r4"]);

    // Bob's cards could be r4 or r5.
    has_poss(&game, None, Player::Bob, 3, &["r4", "r5"]);
    has_poss(&game, None, Player::Bob, 4, &["r4", "r5"]);
}

#[test]
fn it_elims_dark_cards_properly_1() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "b1", "y1", "g1", "k5"],
        ],
        Setup {
            starting: Player::Bob,
            clue_tokens: 4,
            play_stacks: Some(vec![5, 5, 5, 5, 2]),
            variant: TestVariant::Black5,
            init: Box::new(|g| pre_clue(g, Player::Bob, 5, &["black"])),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards g1", "k3");
    take_turn(&mut game, "Alice clues black to Bob");
    take_turn(&mut game, "Bob clues black to Alice (slot 2)");

    // Bob's slot 1 is [k3, k5].
    has_poss(&game, None, Player::Bob, 1, &["k3", "k5"]);
    has_poss(&game, Some(Player::Bob), Player::Bob, 1, &["k3", "k5"]);

    // Alice's slot 2 is exactly k4.
    has_poss(&game, None, Player::Alice, 2, &["k4"]);
    has_poss(&game, Some(Player::Alice), Player::Alice, 2, &["k4"]);
}

#[test]
fn it_elims_dark_cards_properly_2() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "b1", "y1", "g1", "r4"],
        ],
        Setup {
            starting: Player::Bob,
            clue_tokens: 4,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            variant: TestVariant::Black5,
            init: Box::new(|g| {
                pre_clue(g, Player::Alice, 4, &["black"]);
                pre_clue(g, Player::Alice, 5, &["black"]);
                pre_clue(g, Player::Alice, 2, &["4"]);
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards r4", "k3");

    // Alice's slot 2 cannot be a black 4.
    has_poss(
        &game,
        Some(Player::Alice),
        Player::Alice,
        2,
        &["r4", "y4", "g4", "b4"],
    );
}

#[allow(dead_code)]
fn _kind(_: ClueKind) {}
