#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn understands_a_3_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "b1", "g5", "p2"],
            &["p1", "r3", "b5", "b2", "y4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues red to Cathy");
    has_status(&game, Bob, 1, CardStatus::Bluffed);
    has_infs(&game, None, Cathy, 2, &["r1", "r2", "r3"]);
    take_turn_draw(&mut game, "Bob plays b1 (slot 1)", "y5");

    has_infs(&game, None, Cathy, 2, &["r2", "r3"]);
}

#[test]
fn plays_into_a_3_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "r1", "y1", "g5", "p2"],
            &["p1", "r4", "b5", "b2", "y4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays r1 (slot 1)");

    has_infs(&game, None, Bob, 1, &["b2", "b3"]);
}

#[test]
fn plays_into_a_known_3_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g2", "b3"],
            &["p4", "b1", "b1", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    has_infs(&game, None, Alice, 1, &["r1", "y1", "g1", "b1", "p1"]);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays r1 (slot 1)");

    has_infs(&game, None, Bob, 5, &["b2", "b3"]);
}

#[test]
fn receives_a_simple_3_bluff_with_rank_disconnect() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5", "p2"],
            &["p1", "r4", "b5", "b2", "y4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 2, 1, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 2)");
    take_turn_draw(&mut game, "Cathy plays p1", "y5");

    has_infs(&game, None, Alice, 2, &["r3", "g3", "b3", "p3"]);
}

#[test]
fn understands_an_unknown_3_bluff_by_colour() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g2", "b3"],
            &["p4", "b4", "r2", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays y1 (slot 1)");

    // After playing a non-connected card, we know it must be a bluff.
    has_infs(&game, None, Bob, 5, &["b2", "b3"]);
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn understands_an_unknown_3_bluff_by_rank() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g2", "b3"],
            &["p4", "b4", "r2", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays y1 (slot 1)");

    has_infs(&game, None, Bob, 5, &["r3", "y3", "g3", "b3", "p3"]);
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn understands_a_known_hard_self_3_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g4", "b4"],
            &["p4", "b4", "r2", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");
    // Assume a bluff over two blind finesse plays.
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    has_infs(&game, None, Alice, 2, &["r3", "y3", "g3", "b3", "p3"]);
    take_turn(&mut game, "Alice plays y1 (slot 1)");

    // Since we assume a bluff, we don't know which 3 it is.
    has_infs(&game, None, Alice, 2, &["r3", "y3", "g3", "b3", "p3"]);
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn understands_an_unknown_hard_3_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g4", "b3"],
            &["p4", "b4", "r2", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    // Assume a bluff over two blind finesse plays.
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    has_infs(&game, None, Bob, 5, &["r3", "y3", "g3", "b3", "p3"]);
    take_turn(&mut game, "Alice plays b1 (slot 1)");

    // Since we assume a bluff, we don't know which 3 it is.
    has_infs(&game, None, Bob, 5, &["r3", "y3", "g3", "b3", "p3"]);
}

#[test]
fn understands_an_unknown_critical_colour_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "b4", "g3"],
            &["p4", "b1", "b3", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["b4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays y1 (slot 1)");

    has_infs(&game, None, Bob, 4, &["b2", "b3", "b4"]);
}

#[test]
fn understands_a_critical_colour_finesse_when_already_clued() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y2", "b4", "r3", "p4"],
            &["b3", "g4", "p2", "y4"],
            &["p5", "b1", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["b4", "p4"],
            ..Setup::default()
        },
    );
    // Critical save.
    take_turn(&mut game, "Cathy clues 4 to Bob");
    take_turn(&mut game, "Donald clues blue to Bob");

    // Since the critical card was not newly touched, this must be a finesse.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b1"]);
    has_infs(&game, None, Alice, 2, &["b2"]);
    has_infs(&game, None, Cathy, 1, &["b3"]);
}

#[test]
fn understands_an_unknown_critical_colour_bluff_black_suit() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "k4", "g3"],
            &["b4", "b1", "b3", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            variant: TestVariant::Black5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues black to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays y1 (slot 1)");

    has_infs(&game, None, Bob, 4, &["k2", "k3", "k4"]);
}

#[test]
fn understands_a_critical_rank_finesse() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "b4", "g3"],
            &["p4", "b1", "b3", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["b4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Bob");

    // A critical bluff cannot be given by rank. This must be a finesse.
    has_infs(&game, None, Alice, 1, &["b1"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b1"]);
    has_infs(&game, None, Alice, 2, &["b2"]);
    has_infs(&game, None, Alice, 3, &["b3"]);
}

#[test]
fn understands_giving_a_critical_colour_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "b4", "g5", "p2"],
            &["p1", "r4", "b5", "b2", "y4"],
        ],
        Setup {
            discarded: &["r4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Cathy");
    has_status(&game, Bob, 1, CardStatus::Bluffed);
    has_infs(&game, None, Cathy, 2, &["r1", "r2", "r3", "r4"]);
    take_turn_draw(&mut game, "Bob plays b1", "y5");

    has_infs(&game, None, Cathy, 2, &["r2", "r3", "r4"]);
}

#[test]
fn still_understands_a_3_finesse_by_colour_when_the_play_connects() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g2", "b3"],
            &["p4", "b1", "b1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays b1 (slot 1)");

    // After playing a connecting card, we know it must be a finesse.
    has_infs(&game, None, Alice, 2, &["b2"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Bob, 5, &["b3"]);
}

#[test]
fn still_understands_a_3_finesse_by_rank_when_the_play_connects() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "y2", "g2", "g2", "r3"],
            &["p4", "b1", "y1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    has_infs(&game, None, Bob, 5, &["r3", "y3", "g3", "b3", "p3"]);
    take_turn(&mut game, "Alice plays r2 (slot 1)");

    has_infs(&game, None, Bob, 5, &["r3"]);
}

#[test]
fn understands_a_3_bluff_when_the_play_doesn_t_connect() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "y2", "g2", "g2", "r3"],
            &["p4", "b1", "y1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays g1 (slot 1)");

    has_infs(&game, None, Bob, 5, &["r3", "y3", "g3", "b3", "p3"]);
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn still_understands_a_self_3_finesse_when_the_play_connects() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "y2", "g2", "g2", "b3"],
            &["p4", "b1", "y1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays r2 (slot 1)");

    has_infs(&game, None, Alice, 2, &["r3"]);
}

#[test]
fn understands_a_self_3_bluff_when_the_possible_finesse_doesn_t_connect() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "y2", "g2", "g2", "b3"],
            &["p4", "b1", "y1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays y1 (slot 1)");

    has_infs(&game, None, Alice, 2, &["r3", "y3", "g3", "b3", "p3"]);
    has_status(&game, Alice, 3, CardStatus::None);
}

#[test]
fn still_understands_a_clandestine_3_finesse_when_the_play_connects() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "g4", "g2", "g2", "y3"],
            &["p4", "b1", "y1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays r2 (slot 1)");

    has_infs(&game, None, Alice, 2, &["y2"]);
}

#[test]
fn still_understands_a_clandestine_3_finesse_when_the_play_connects_2_away() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "g4", "g2", "g2", "y3"],
            &["p4", "b1", "y1", "b4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays r2 (slot 1)");

    has_infs(&game, None, Alice, 2, &["y1"]);
    has_infs(&game, None, Alice, 3, &["y2"]);
}

#[test]
fn still_understands_receiving_a_clandestine_3_finesse_when_the_play_connects() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p3", "p4", "g2", "g2", "p4"],
            &["r2", "y1", "y2", "g4", "b4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 2)");
    take_turn_draw(&mut game, "Cathy plays r2", "r5");

    has_infs(&game, None, Alice, 2, &["r3", "y3"]);
}

#[test]
fn does_not_interpret_colour_connect_with_rank_disconnect_as_a_3_bluff() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "g2", "g4", "y4", "b4"],
            &["g5", "y5", "b3", "p3", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    take_turn(&mut game, "Alice plays r1 (slot 1)");

    // Since red connects to red, Alice must assume a double finesse (she also has r2 on slot 2), not a 3-bluff.
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Alice, 2, &["r2"]);
    has_infs(&game, None, Bob, 1, &["r3"]);
}

#[test]
fn still_understands_receiving_colour_connect_with_rank_disconnect() {
    let mut game = setup(
        hgroup(13),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "g2", "g4", "y4", "b4"],
            &["r1", "r2", "b3", "p3", "y3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 1)");
    take_turn_draw(&mut game, "Cathy plays r1", "b4");

    // Alice must wait for a double finesse.
    has_status(&game, Cathy, 2, CardStatus::Finessed);
    has_infs(&game, None, Cathy, 2, &["r2"]);
    has_infs(&game, None, Alice, 1, &["r2", "r3"]);
}
