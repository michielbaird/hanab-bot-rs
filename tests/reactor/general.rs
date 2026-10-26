use super::common::*;
use scala_bot::basics::action::{Action, ClueAction, DiscardAction};
use scala_bot::basics::clue::{BaseClue, ClueKind};
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, PerformAction};
use scala_bot::reactor::ReactorGame;
use scala_bot::reactor::state_eval::eval_action_impl;

use Player::*;

#[test]
fn it_elims_from_focus() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "g2", "r2", "r3", "g5"],
            &["p4", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 1,2)");

    // Alice's slot 1 should be known r5.
    has_infs(&game, None, Alice, 1, &["r5"]);

    // Alice's slot 2 should be known trash.
    let hand = &game.state.hands[Alice.ordinal()];
    assert!(
        game.common
            .thinks_trash(&game, Alice.ordinal())
            .contains(&hand[1])
    );
}

#[test]
fn it_understands_a_stable_clue_to_cathy() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r4", "r4", "y4", "y4"],
            &["g1", "g4", "g4", "b4", "b4"],
        ],
        Setup {
            // Bob's slot 1 is clued with 1.
            init: Box::new(|g| pre_clue(g, Bob, 1, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues green to Cathy");

    // Cathy is called to play g1.
    has_status(&game, Cathy, 1, CardStatus::DragToPlay);
}

#[test]
fn it_understands_a_reverse_reactive_clue() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r4", "y4", "y4"],
            &["g4", "g1", "g4", "b4", "b4"],
        ],
        Setup {
            clue_tokens: 7,
            // Bob's slot 2 is clued with 1.
            init: Box::new(|g| pre_clue(g, Bob, 2, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 4 to Bob");
    // Cathy is called to play g1.
    has_status(&game, Cathy, 2, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Bob plays b1", "y3");

    assert!(
        game.common
            .obvious_playables(&game, Cathy.ordinal())
            .contains(&game.state.hands[Cathy.ordinal()][1])
    );
}

#[test]
fn it_doesnt_give_a_bad_reverse_reactive_clue() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r1", "r4", "y4", "y5"],
            &["y1", "g4", "g4", "b4", "b4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 5 to Bob");
    take_turn(&mut game, "Alice plays g1 (slot 4)"); // targeting Bob's b1
    take_turn(&mut game, "Bob clues green to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "b1");

    // We cannot give 4 to Bob as a reverse reactive, since Cathy would play b1 to react into r1 and Bob is already playing b1.
    let clue = ClueAction {
        giver: Alice.ordinal(),
        target: Bob.ordinal(),
        list: vec![6, 7],
        clue: BaseClue::new(ClueKind::Rank, 4),
    };
    assert!(eval_action_impl(&game, &Action::Clue(clue)) < 8.0);
}

#[test]
fn it_understands_targeting_dupes() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "r4", "r4", "y4", "y5"],
            &["g4", "g1", "g4", "b4", "b4"],
        ],
        Setup {
            starting: Cathy,
            // Bob's slots 2 and 3 are clued with red.
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["red"]);
                pre_clue(g, Bob, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    // 4 + 2 = 1
    take_turn(&mut game, "Cathy clues blue to Bob");
    has_status(&game, Alice, 4, CardStatus::DragToPlay);
    take_turn(&mut game, "Alice plays r1 (slot 4)");

    has_status(&game, Bob, 2, CardStatus::CalledToDiscard);
}

#[test]
fn it_understands_a_known_delayed_stable_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "y5", "g4", "b4", "b4"],
            &["b1", "r1", "r4", "y4", "y4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 0, 1, 0, 0]),
            init: Box::new(|g| {
                fully_known(g, Alice, 1, "r1");
                fully_known(g, Alice, 2, "g2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob");

    has_status(&game, Bob, 1, CardStatus::DragToPlay);

    // We should play g2 into it.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
}

#[test]
fn it_understands_an_unknown_delayed_stable_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "y5", "g4", "b4", "b4"],
            &["b1", "r1", "r4", "y4", "y4"],
        ],
        Setup {
            starting: Cathy,
            init: Box::new(|g| pre_clue(g, Alice, 1, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob");

    has_status(&game, Bob, 1, CardStatus::DragToPlay);

    // We should play our 1 into it as g1.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
    has_infs(&game, None, Alice, 1, &["g1"]);
}

#[test]
fn it_doesnt_give_a_bad_connecting_clue() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "y1", "g1", "g2", "p2"],
            &["y1", "p3", "g5", "p5", "r5"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 1, 1]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 4, &["2"]);
                pre_clue(g, Bob, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );

    let clue = ClueAction {
        giver: Alice.ordinal(),
        target: Cathy.ordinal(),
        list: vec![12],
        clue: BaseClue::new(ClueKind::Colour, 2),
    };

    let new_game = game.simulate_clue(&clue, false, true);
    assert_eq!(
        new_game.last_move(),
        Some(Interp::Clue(ClueInterp::Mistake))
    );
}

#[test]
fn it_understands_bob_wont_react_if_alternative_is_on_them() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "r2", "g1", "g2", "p2"],
            &["r3", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| pre_clue(g, Cathy, 3, &["green"])),
            clue_tokens: 8,
            ..Setup::default()
        },
    );
    // Trying to get a finesse, but Bob doesn't see another clue for Alice to give.
    take_turn(&mut game, "Alice clues 5 to Cathy");

    has_status(&game, Bob, 2, CardStatus::None);
}

#[test]
fn it_discards_zcs() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "r2", "g1", "g2", "p2"],
            &["r1", "p4", "g4", "y5", "r4"],
        ],
        Setup {
            clue_tokens: 1,
            ..Setup::default()
        },
    );
    // Taking the team down to 0 clues - Bob writes ZCS on y4, Cathy writes ZCS on p4.
    take_turn(&mut game, "Alice clues 5 to Cathy");
    take_turn_draw(&mut game, "Bob plays g1", "b1");
    take_turn_draw(&mut game, "Cathy plays r1", "y3");
    take_turn(&mut game, "Alice discards p4 (slot 1)");
    // Bob's chop should be slot 2.
    assert_eq!(
        game.chop(Bob.ordinal()),
        Some(game.state.hands[Bob.ordinal()][1])
    );
    take_turn_draw(&mut game, "Bob discards y4", "r3");

    assert_eq!(game.zcs_turn(), None);
    // Cathy's chop should be slot 1.
    assert_eq!(
        game.chop(Cathy.ordinal()),
        Some(game.state.hands[Cathy.ordinal()][0])
    );
}

#[test]
fn it_interprets_a_gd() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "r2", "g1", "g2", "p2"],
            &["r3", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            init: Box::new(|g| fully_known(g, Bob, 1, "y1")),
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
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "r2", "g1", "g2", "p2"],
            &["y1", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            init: Box::new(|g| fully_known(g, Cathy, 1, "y1")),
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
fn it_interprets_a_sarcastic() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "r2", "g1", "g2", "p2"],
            &["r3", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            init: Box::new(|g| fully_known(g, Bob, 2, "r2")),
            starting: Bob,
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards r2", "y4");
    take_turn(&mut game, "Cathy clues blue to Alice (slots 2,3,4)");
    take_turn(&mut game, "Alice plays r1 (slot 1)");

    has_status(&game, Alice, 5, CardStatus::Sarcastic);
    has_infs(&game, None, Alice, 5, &["r2"]);
    assert!(
        game.common
            .obvious_playables(&game, Alice.ordinal())
            .contains(&0)
    );
}

#[test]
fn it_doesnt_perform_a_bad_gd() {
    let game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "r2", "g1", "g2", "p2"],
            &["r3", "p4", "g5", "y4", "r4"],
        ],
        Setup {
            init: Box::new(|g| {
                fully_known(g, Alice, 5, "g1");
                pre_clue(g, Bob, 3, &["green"]);
                pre_clue(g, Bob, 4, &["green"]);
            }),
            clue_tokens: 6,
            ..Setup::default()
        },
    );

    let action = Action::Discard(DiscardAction {
        player_index: 0,
        order: Alice.ordinal(),
        id: Some(game.state.expand_short("g1").unwrap()),
        failed: false,
    });
    let result = eval_action_impl(&game, &action);

    // Discarding g1 in slot 5 is a bad GD.
    assert_eq!(result, -100.0);
}
