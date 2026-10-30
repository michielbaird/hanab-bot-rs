//! Port of `test/endgame.scala`.
//!
//! The H-Group variant of the rainbow test is ported in M6.

mod common;

use common::*;
use hanab_bot::basics::{Game, PerformAction};
use hanab_bot::endgame::EndgameSolver;
use hanab_bot::frac::Frac;
use hanab_bot::logger::{self, LogLevel};

use Player::*;

/// (The Scala tests use the default 3 second timeout; the Rust solver is slower, so the tests allow
/// more time to stay deterministic on slow machines. See HANDOFF.md, "Performance".)
fn solve(game: &Game, monte_carlo: bool) -> (PerformAction, Frac) {
    logger::set_level(LogLevel::Off);
    let search = |threads: usize| {
        EndgameSolver::new(monte_carlo)
            .with_timeout(std::time::Duration::from_secs(60))
            .with_threads(threads)
            .solve(game, None)
            .unwrap_or_else(|msg| panic!("Game should be winnable! {msg}"))
    };

    // More threads only make the search faster: it finds the same move.
    let result = search(1);
    assert_eq!(search(4), result, "the answer depends on the threads");
    result
}

fn is_clue(p: PerformAction) -> bool {
    p.is_clue()
}

#[test]
fn it_clues_to_start_b45_endgame() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "y1", "g1", "b5"],
            &["g1", "b1", "b1", "r5"],
            &["b4", "p1", "p1", "r1"],
        ],
        Setup {
            play_stacks: Some(vec![4, 4, 5, 3, 5]),
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "g4", "b2", "b3", "p2", "p3", "p4",
            ], // Missing: r1, y1, r4, y4
            init: Box::new(|g| {
                fully_known(g, Alice, 1, "y5");
                fully_known(g, Bob, 1, "b4");
                fully_known(g, Bob, 4, "b5");
                fully_known(g, Cathy, 4, "r5");
                fully_known(g, Donald, 1, "b4");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 1);

    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    assert!(is_clue(perform));
}

#[test]
fn it_clues_to_start_endgame_on_a_double_player() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g5", "y4", "g1", "r1"],
            &["g1", "b1", "b1", "r1"],
            &["y4", "p1", "p1", "y5"],
        ],
        Setup {
            play_stacks: Some(vec![5, 3, 4, 5, 5]),
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "b2", "b3", "p2", "p3", "p4",
            ], // Missing: y1, y1, r4, g4, b4
            init: Box::new(|g| {
                fully_known(g, Bob, 1, "g5");
                fully_known(g, Bob, 2, "y4");
                fully_known(g, Donald, 1, "y4");
                fully_known(g, Donald, 4, "y5");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 1);

    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    assert!(is_clue(perform));
}

#[test]
fn it_plays_to_start_simple_endgame() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "r1", "y1", "b5"],
            &["g1", "g1", "y1", "p1"],
            &["b1", "b1", "p1", "r5"],
        ],
        Setup {
            play_stacks: Some(vec![3, 5, 5, 4, 5]),
            discarded: &[
                "r3", "y3", "y4", "g3", "g4", "b2", "b3", "b4", "p2", "p3", "p4",
            ], // Missing: r2, y2, b2
            init: Box::new(|g| {
                fully_known(g, Alice, 1, "r4");
                fully_known(g, Alice, 2, "r4");
                fully_known(g, Bob, 4, "b5");
                fully_known(g, Donald, 4, "r5");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 1);

    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    assert_eq!(
        perform,
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn it_plays_to_start_endgame_when_other_has_dupes() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "p4", "b1", "p4"],
            &["r1", "y1", "g1", "p1"],
            &["r1", "y1", "g1", "p5"],
        ],
        Setup {
            play_stacks: Some(vec![5, 5, 5, 5, 2]),
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "b2", "b3", "b4", "p2", "p3",
            ], // Missing: p1, r4, y4, g4
            init: Box::new(|g| {
                fully_known(g, Alice, 1, "p3");
                fully_known(g, Bob, 2, "p4");
                fully_known(g, Bob, 4, "p4");
                fully_known(g, Donald, 4, "p5");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 1);

    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    // Alice should play p3.
    assert_eq!(
        perform,
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn it_plays_to_start_a_complex_endgame_where_all_cards_are_seen() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "g1", "p5", "p2"],
            &["g1", "b1", "r4", "r1", "g5"],
        ],
        Setup {
            play_stacks: Some(vec![3, 5, 4, 5, 1]),
            discarded: &[
                "r2", "r3", "y2", "y3", "y4", "g2", "g3", "g4", "b2", "b3", "b4", "p2", "p3",
            ], // Missing: y1, y1, p1, p1, p4
            init: Box::new(|g| {
                fully_known(g, Alice, 2, "p3");
                fully_known(g, Alice, 3, "p4");
                fully_known(g, Alice, 4, "r5");
                fully_known(g, Alice, 5, "r4");
                fully_known(g, Bob, 4, "p5");
                fully_known(g, Bob, 5, "p2");
                fully_known(g, Cathy, 5, "g5");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 4);

    // Alice plays r4 (3 left), Bob plays p2 (2 left), Cathy stalls
    // Alice plays p3 (1 left), Bob stalls, Cathy stalls
    // Alice plays p4 (0 left), Bob plays p5, Cathy plays g5
    // Alice plays r5.
    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    // Alice should play r4.
    assert_eq!(
        perform,
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn it_calculates_basic_winrate_correctly() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "g1", "y1", "r4"],
            &["b1", "r1", "g1", "y1", "r5"],
        ],
        Setup {
            play_stacks: Some(vec![2, 4, 5, 5, 5]),
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "b2", "b3", "b4", "p2", "p3", "p4",
            ], // Missing: p1, p1, r4, y4, g4, y5
            clue_tokens: 0,
            init: Box::new(|g| {
                fully_known(g, Alice, 5, "r3");
                fully_known(g, Bob, 5, "r4");
                fully_known(g, Cathy, 5, "r5");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 2);

    let (perform, winrate) = solve(&game, false);
    // We win if either Bob draws y5 (and we clue), or we draw y5 (and yolo). There are 6 locations that y5 could be.
    assert_eq!(winrate, Frac::new(1, 3));
    // Alice should play r3.
    assert_eq!(
        perform,
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn it_wins_an_unconventional_rainbow_endgame() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "m1", "m5", "y5"],
            &["b1", "r1", "g1", "y1", "m4"],
        ],
        Setup {
            play_stacks: Some(vec![5, 4, 5, 5, 3]),
            clue_tokens: 1,
            variant: TestVariant::Rainbow5,
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "b2", "b3", "b4", "m2", "m3", "m4",
            ], // Missing: y1, g1, m1, r4, y4, g4
            init: Box::new(|g| {
                pre_clue(g, Bob, 4, &["5"]);
                pre_clue(g, Bob, 5, &["5"]);
                pre_clue(g, Cathy, 5, &["4", "yellow", "blue"]);
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 1);

    let (perform, winrate) = solve(&game, false);
    assert_eq!(winrate, Frac::ONE);
    // Anything but a yellow clue to Bob (which would touch y5 and m5)
    assert!(matches!(perform, PerformAction::Colour { target: 1, value } if value != 1));
}

#[test]
fn it_completes_an_early_endgame() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "y4", "p4", "y3", "b5"],
            &["p1", "g1", "g2", "b1", "g3"],
        ],
        Setup {
            play_stacks: Some(vec![5, 5, 5, 3, 4]),
            discarded: &["r1", "r1", "y1", "b1", "b4", "p1", "p3"],
            // Missing: r2, y1, g4, b3, b4, p5
            clue_tokens: 5,
            init: Box::new(|g| {
                fully_known(g, Alice, 2, "r3");
                fully_known(g, Alice, 3, "r4");
                fully_known(g, Alice, 4, "p2");
                fully_known(g, Alice, 5, "g1");
                pre_clue(g, Bob, 5, &["5"]);
                pre_clue(g, Cathy, 4, &["blue"]);
                fully_known(g, Cathy, 5, "g3");
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 6);

    let (_, winrate) = solve(&game, false);
    // We only lose if b4 is on the bottom.
    assert!(
        winrate > Frac::new(2, 3),
        "got winrate {winrate}, expected > 2/3"
    );
}

#[test]
fn ends_an_endgame_as_quickly_as_possible() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["o5", "o1", "g4", "r1", "y1"],
            &["g2", "g3", "b4", "b2", "b5"],
        ],
        Setup {
            play_stacks: Some(vec![5, 5, 5, 4, 4]),
            discarded: &["y1", "y2", "g1", "g1", "o1", "o2", "o3", "o4"],
            // Missing: r2, y4, y3, b3, b1, b1, r4
            clue_tokens: 5,
            variant: TestVariant::Omni5,
            init: Box::new(|g| fully_known(g, Cathy, 5, "b5")),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 4);

    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    assert!(matches!(perform, PerformAction::Colour { target: 1, .. }));
}

#[test]
fn tries_bombing_slot_1_in_the_final_round() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "y1", "g1", "b1", "p1"],
            &["r1", "y1", "g1", "b1", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![4, 4, 5, 5, 5]),
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "b2", "b3", "p2", "p3", "p4",
            ], // Missing: r1, r4, y5, y4, g4, b4
            clue_tokens: 0,
            init: Box::new(|g| fully_known(g, Bob, 1, "r5")),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Cathy discards p1", "r1");

    assert_eq!(game.state.cards_left, 0);

    let (perform, winrate) = solve(&game, true);
    assert!(winrate > Frac::ZERO);
    assert_eq!(
        perform,
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn it_wins_an_unconventional_rainbow_endgame_in_h() {
    let game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "m1", "m5", "y5"],
            &["b1", "r1", "g1", "y1", "m4"],
        ],
        Setup {
            play_stacks: Some(vec![5, 4, 5, 5, 3]),
            clue_tokens: 1,
            variant: TestVariant::Rainbow5,
            discarded: &[
                "r2", "r3", "y2", "y3", "g2", "g3", "b2", "b3", "b4", "m2", "m3", "m4",
            ], // Missing: y1, g1, m1, r4, y4, g4
            init: Box::new(|g| {
                pre_clue(g, Bob, 4, &["5"]);
                pre_clue(g, Bob, 5, &["5"]);
                pre_clue(g, Cathy, 5, &["4", "yellow", "blue"]);
            }),
            ..Setup::default()
        },
    );

    assert_eq!(game.state.cards_left, 1);

    let (perform, winrate) = solve(&game, true);
    assert_eq!(winrate, Frac::ONE);
    assert!(matches!(
        perform,
        PerformAction::Colour { target: 1, value } if value != Colour::Yellow.ordinal()
    ));
}
