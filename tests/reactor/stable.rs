use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};

use Player::*;

#[test]
fn it_understands_a_ref_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "r3", "g5"],
            &["p4", "b5", "p2", "b1", "g4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues green to Bob");

    has_status(&game, Bob, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Bob, 1, &["r1", "y1", "b1", "p1"]);
}

#[test]
fn it_understands_a_gapped_ref_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "b1", "p2", "b5", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues purple to Bob");

    has_status(&game, Bob, 2, CardStatus::DragToPlay);
    has_infs(&game, None, Bob, 2, &["r1", "y1", "g1", "b1"]);
}

#[test]
fn it_understands_a_chop_ref_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b2", "p2", "b5", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues blue to Bob");

    has_infs(&game, None, Bob, 1, &["b1"]);
}

#[test]
fn it_understands_a_ref_discard() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "b5", "g3"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 4 to Bob");

    has_status(&game, Bob, 2, CardStatus::CalledToDiscard);
}

#[test]
fn it_gives_a_ref_discard() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "b5", "g3"],
            &["b3", "g2", "r2", "r3", "g5"],
        ],
        Setup::default(),
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
fn it_interprets_a_direct_rank() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "b1", "b1", "u4"],
            &["y1", "y1", "g1", "g1", "u3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 4, 5, 2, 0]),
            variant: TestVariant::Null5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");

    has_status(&game, Alice, 3, CardStatus::None);
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][1])
    );
}

#[test]
fn eliminates_direct_ranks_from_focus() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "r5", "g3"],
            &["b5", "y4", "g2", "r4", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 1, 0, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,3)");

    has_status(&game, Alice, 4, CardStatus::None);
    has_infs(&game, None, Alice, 2, &["g1"]);

    // Alice's slot 3 should be trash
    let trash = game.common.thinks_trash(&game, Alice.ordinal());
    assert!(trash.contains(&game.state.hands[Alice.ordinal()][2]));
}

#[test]
fn it_understands_a_lock() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "b5", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 4 to Bob");

    assert!(game.common.obvious_locked(&game, Bob.ordinal()));
}

#[test]
fn doesnt_lock_an_already_locked_player() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "p2", "p2", "b5", "g4"],
            &["b1", "g2", "r2", "r3", "g5"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 4 to Bob"); // lock
    take_turn(&mut game, "Bob clues green to Cathy");
    take_turn_draw(&mut game, "Cathy plays b1", "y5");
    take_turn(&mut game, "Alice clues 5 to Bob"); // lock again??

    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn it_doesnt_focus_the_wrong_card_for_the_last_id() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "b1", "p1"],
            &["r1", "y1", "g1", "b1", "p1"],
        ],
        Setup {
            play_stacks: Some(vec![3, 3, 3, 3, 2]),
            starting: Cathy,
            init: Box::new(|g| pre_clue(g, Alice, 5, &["purple"])),
            ..Setup::default()
        },
    );
    // Although Alice could play slot 2, she should play slot 5 first.
    take_turn(&mut game, "Cathy clues 3 to Alice (slots 2,5)");

    assert_eq!(game.take_action(), PerformAction::Play { target: 0 });
}

#[test]
fn it_interprets_a_fix() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "p2", "p2", "b5", "g4"],
            &["r1", "g2", "r2", "r3", "g5"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    take_turn(&mut game, "Bob clues green to Cathy");
    take_turn_draw(&mut game, "Cathy plays r1", "g3");
    take_turn(&mut game, "Alice clues red to Bob");

    // Bob's slot 1 should be known trash.
    assert!(
        game.common
            .thinks_trash(&game, Bob.ordinal())
            .contains(&game.state.hands[Bob.ordinal()][0])
    );
    has_infs(&game, None, Bob, 1, &["r1"]);
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Fix)));
}

#[test]
fn it_interprets_a_reveal_through_an_unknown_card() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "y2", "g2", "b2", "b4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            init: Box::new(|g| pre_clue(g, Bob, 5, &["4"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob"); // locked
    take_turn(&mut game, "Bob clues purple to Alice (slot 5)"); // ref play on slot 4
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should play slot 4 as g1, instead of trying to play slot 3 to sacrifice b4.
    has_status(&game, Alice, 3, CardStatus::None);
    has_infs(&game, None, Alice, 4, &["g1"]);
}

#[test]
fn it_interprets_a_1_away_reveal_through_an_unknown_card() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "y3", "g2", "b2", "b4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            init: Box::new(|g| pre_clue(g, Bob, 5, &["4"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob"); // locked
    take_turn(&mut game, "Bob clues purple to Alice (slot 5)"); // ref play on slot 4
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should play slot 4 as g1, instead of trying to play slot 3 to sacrifice b4.
    has_status(&game, Alice, 3, CardStatus::None);
    has_infs(&game, None, Alice, 4, &["g1"]);
}

#[test]
fn it_interprets_a_1_away_reveal_through_a_known_card() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "y3", "g2", "b2", "r4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            init: Box::new(|g| {
                fully_known(g, Alice, 3, "b1");
                pre_clue(g, Bob, 3, &["green"]);
                pre_clue(g, Bob, 4, &["blue"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");

    // Stable, not 5 + 3 = 3
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Reveal)));
    has_status(&game, Alice, 5, CardStatus::None);
}

#[test]
fn doesnt_give_a_false_1_away_reveal() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g1", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Cathy"); // locked
    take_turn(&mut game, "Cathy clues 1 to Bob"); // direct 1
    take_turn(&mut game, "Alice clues purple to Cathy");

    // Bob thinks they can connect by playing p1, but it's actually g1.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesnt_break_pink_promise() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["i2", "i5", "g3", "g3", "r3"],
            &["r4", "y4", "g4", "b4", "r4"],
        ],
        Setup {
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );

    // Bob should clue 5 and not 2.
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
}

#[test]
fn understands_a_tempo_clue() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 1, &["2"]);
                pre_clue(g, Alice, 2, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Alice (slots 1,2)");

    // Only slot 1 is playable.
    assert_eq!(
        game.common.obvious_playables(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn reacts_to_a_reclue_that_isnt_tempo() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "r2", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 1, &["2"]);
                pre_clue(g, Bob, 2, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");

    // We need to react, since Bob will bomb y2.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Reactive)));
}
