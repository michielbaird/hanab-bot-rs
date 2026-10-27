#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::interp::{ClueInterp, Interp};
use hanab_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn interprets_a_fill_in_bluff() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y4", "y4", "g3"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    has_status(&game, Bob, 1, CardStatus::Bluffed);
    take_turn_draw(&mut game, "Bob plays b1", "p1");
    has_status(&game, Bob, 2, CardStatus::None);
}

#[test]
fn receives_a_fill_in_bluff() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn interprets_a_self_colour_bluff() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y4", "y4", "g2"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues green to Bob");
    has_status(&game, Bob, 1, CardStatus::Bluffed);
    take_turn_draw(&mut game, "Bob plays b1", "p1");
    has_status(&game, Bob, 2, CardStatus::None);
}

#[test]
fn receives_a_self_colour_bluff() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    has_status(&game, Alice, 2, CardStatus::None);
}

#[test]
fn prefers_to_scb_over_direct() {
    let game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "y4", "g4", "b2", "b4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 7,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            init: Box::new(|g| {
                fully_known(g, Bob, 2, "y4");
                fully_known(g, Bob, 3, "g4");
                pre_clue(g, Bob, 4, &["2"]);
                pre_clue(g, Bob, 5, &["4"]);
            }),
            ..Setup::default()
        },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Colour {
            target: Bob.ordinal(),
            value: Colour::Blue.ordinal()
        }
    );
}

#[test]
fn interprets_a_finesse_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "b4", "y4", "g2", "g3"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 4, &["green"]);
                pre_clue(g, Bob, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    has_status(&game, Bob, 1, CardStatus::Finessed);
    take_turn_draw(&mut game, "Bob plays g1", "p1");
    has_infs(&game, None, Bob, 4, &["g2"]);
}

#[test]
fn receives_a_finesse_prompt() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 4, &["green"]);
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    has_infs(&game, None, Alice, 4, &["g2"]);
}

#[test]
fn interprets_a_bluff_over_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y4", "g2", "g3"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 4, &["green"]);
                pre_clue(g, Bob, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    has_status(&game, Bob, 1, CardStatus::Bluffed);
    take_turn_draw(&mut game, "Bob plays b1", "p1");
    has_infs(&game, None, Bob, 4, &["g1", "g2", "g4", "g5"]);
}

#[test]
fn doesn_t_prompt_in_the_wrong_order() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "b4", "y4", "g1", "g3"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 4, &["green"]);
                pre_clue(g, Bob, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn receives_a_bluff_over_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b4", "y4", "y4", "g4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 4, &["green"]);
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    has_infs(&game, None, Alice, 4, &["g1", "g2", "g4", "g5"]);
}

#[test]
fn receives_a_complex_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b4", "y4", "y4", "b4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                fully_known(g, Alice, 3, "g1");
                pre_clue(g, Alice, 4, &["green"]);
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
    take_turn(&mut game, "Alice plays g1 (slot 3)");
    take_turn_draw(&mut game, "Bob discards g4", "r4");
    take_turn(&mut game, "Alice plays g2 (slot 2)");
    has_infs(&game, None, Alice, 4, &["g3"]);
}

#[test]
fn prompts_if_all_possible() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b4", "y4", "y4", "b4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 3, &["green"]);
                pre_clue(g, Alice, 4, &["green"]);
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 5)");
    has_infs(&game, None, Alice, 4, &["g1"]);
    has_infs(&game, None, Alice, 3, &["g2"]);
}

#[test]
fn doesn_t_give_bad_prompts() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "b4", "g1", "g2", "g3"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 3, &["green"]);
                pre_clue(g, Bob, 4, &["green"]);
                pre_clue(g, Bob, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn receives_a_complex_bluff_over_finesse() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b4", "y4", "y4", "b4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                fully_known(g, Alice, 3, "g1");
                pre_clue(g, Alice, 4, &["green"]);
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][2]
        }
    );
    take_turn(&mut game, "Alice plays g1 (slot 3)");
    take_turn_draw(&mut game, "Bob discards g4", "r4");
    take_turn(&mut game, "Alice plays b1 (slot 2)");
    has_infs(&game, None, Alice, 4, &["g2", "g3", "g5"]);
}

#[test]
fn receives_a_no_info_double_bluff() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "b4", "y4", "y4", "b4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 5)");
    has_status(&game, Alice, 1, CardStatus::DragToPlay);
    has_status(&game, Alice, 2, CardStatus::DragToPlay);
    // Only slot 1 is playable.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn doesn_t_give_a_bad_no_info_bluff() {
    let mut game = setup(
        ref_sieve(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y4", "y4", "g4"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["green"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues green to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
