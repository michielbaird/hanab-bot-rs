use super::common::*;
use scala_bot::basics::interp::{ClueInterp, Interp};
use scala_bot::basics::{CardStatus, Game, Identity, PerformAction};

use Player::*;

fn ids(game: &Game, shorts: &[&str]) -> Vec<Identity> {
    shorts
        .iter()
        .map(|s| game.state.expand_short(s).unwrap())
        .collect()
}

#[test]
fn it_understands_a_reactive_play_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 5 to Cathy");
    has_status(&game, Bob, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Bob, 1, &["r1", "y1", "b1", "p1"]);
    take_turn_draw(&mut game, "Bob plays b1", "p1");

    has_status(&game, Cathy, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Cathy, 1, &["r1", "y1", "g1", "b2", "p1"]);
}

#[test]
fn it_reacts_to_a_reactive_play_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");

    has_status(&game, Alice, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Alice, 1, &["r1", "y1", "g1", "p1"]);

    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn it_receives_a_reactive_play_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Alice (slot 3)");
    take_turn_draw(&mut game, "Cathy plays g1", "y3");

    has_status(&game, Alice, 2, CardStatus::DragToPlay);
    has_infs(&game, None, Alice, 2, &["r1", "y1", "g2", "b1", "p1"]);
}

#[test]
fn it_reacts_to_a_reverse_reactive_play_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "g2", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    take_turn(&mut game, "Alice discards r3 (slot 2)"); // Bob's b1 is called to play
    take_turn(&mut game, "Bob clues blue to Cathy");

    take_turn_draw(&mut game, "Cathy plays g1", "y3");
    take_turn(&mut game, "Alice clues 1 to Bob"); // Reverse reactive, getting Cathy's g2 and Bob's b2 (4 + 2 = 1)

    has_status(&game, Cathy, 4, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Bob plays b1", "y5");

    has_infs(&game, None, Cathy, 4, &["r1", "y1", "g2", "p1"]);
}

#[test]
fn it_elims_a_reactive_play_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "g2", "r2", "y3", "g5"],
            &["g3", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Bob, 3, &["red"]);
                pre_clue(g, Bob, 4, &["3"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 4 to Cathy");
    has_status(&game, Bob, 1, CardStatus::DragToPlay);
    take_turn_draw(&mut game, "Bob plays r1", "p1");

    let (common, state) = (&game.common, &game.state);

    has_status(&game, Cathy, 4, CardStatus::DragToPlay);
    let cathy = &state.hands[Cathy.ordinal()];

    // Since Bob cannot play a known 3, Cathy can't write !playable on slot 1.
    assert!(
        ids(&game, &["r1", "y1", "g1", "b1", "p1"])
            .iter()
            .all(|&id| common.thoughts[cathy[0]].inferred.contains(id))
    );

    // Since Bob cannot play r1 onto r1, Cathy can't write !r1 on slot 2.
    assert!(
        ids(&game, &["y1", "g1", "b1", "p1"])
            .iter()
            .all(|&id| !common.thoughts[cathy[1]].inferred.contains(id))
    );
    assert!(
        common.thoughts[cathy[1]]
            .inferred
            .contains(state.expand_short("r1").unwrap())
    );

    // Bob can play slot 2, so Cathy can write !playable on slot 3.
    assert!(
        ids(&game, &["r1", "y1", "g1", "b1", "p1"])
            .iter()
            .all(|&id| !common.thoughts[cathy[2]].inferred.contains(id))
    );
}

#[test]
fn it_understands_a_reactive_dc_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues blue to Cathy");
    has_status(&game, Bob, 1, CardStatus::CalledToDiscard);
    assert!(
        game.common
            .thinks_trash(&game, Bob.ordinal())
            .contains(&game.state.hands[Bob.ordinal()][0])
    );
    take_turn_draw(&mut game, "Bob discards r3 (slot 1)", "p3");

    has_status(&game, Cathy, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Cathy, 1, &["r1", "y1", "g1", "p1"]);
}

#[test]
fn it_elims_a_reactive_dc_play() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "g5", "y3"],
            &["b3", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Bob, 3, &["red"]);
                pre_clue(g, Bob, 4, &["5"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues green to Cathy");
    has_status(&game, Bob, 1, CardStatus::CalledToDiscard);
    take_turn_draw(&mut game, "Bob discards b1", "b4");

    let (common, state) = (&game.common, &game.state);

    has_status(&game, Cathy, 4, CardStatus::DragToPlay);
    let cathy = &state.hands[Cathy.ordinal()];

    // Since Bob cannot discard a known 5, Cathy can't write !playable on slot 1.
    assert!(
        ids(&game, &["r1", "r2", "y1", "b1"])
            .iter()
            .all(|&id| common.thoughts[cathy[0]].inferred.contains(id))
    );

    // Bob can discard the other slots, so Cathy can write !playable on slots 2 and 3.
    assert!(
        ids(&game, &["r2", "y2", "g2", "b1", "p1"])
            .iter()
            .all(|&id| !common.thoughts[cathy[1]].inferred.contains(id))
    );
    assert!(
        ids(&game, &["r2", "y2", "g2", "b1", "p1"])
            .iter()
            .all(|&id| !common.thoughts[cathy[2]].inferred.contains(id))
    );
}

#[test]
fn it_elims_a_reactive_play_dc() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "g5", "y3"],
            &["b1", "b5", "p2", "b3", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 0, 0]),
            clue_tokens: 7,
            starting: Bob,
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["red"]);
                pre_clue(g, Alice, 3, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy plays b1", "r4");

    has_status(&game, Alice, 3, CardStatus::CalledToDiscard);

    let alice = game.state.hands[Alice.ordinal()].clone();

    // Alice's slot 1 can still be trash, since Cathy targeted clued trash.
    assert!(
        game.common.thoughts[alice[0]]
            .inferred
            .iter()
            .any(|id| game.state.is_basic_trash(id))
    );

    // However, ALice's slot 2 is known !trash.
    assert!(
        game.common.thoughts[alice[1]]
            .inferred
            .iter()
            .all(|id| !game.state.is_basic_trash(id))
    );
}

#[test]
fn it_reacts_to_a_reactive_finesse() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");

    // We should play slot 1 to target Bob's r2.
    has_status(&game, Alice, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Alice, 1, &["r1"]);

    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn it_doesnt_play_target_an_unclued_dupe() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 1, 0, 0]),
            clue_tokens: 7,
            init: Box::new(|g| pre_clue(g, Bob, 4, &["red"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should discard slot 5 (so that Bob plays slot 2).
    has_status(&game, Alice, 5, CardStatus::CalledToDiscard);
}

#[test]
fn it_doesnt_play_target_a_discarding_dupe() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "g2", "y3", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");
    take_turn(&mut game, "Alice plays r2 (slot 3)"); // Targeting discard on r3
    has_status(&game, Bob, 1, CardStatus::CalledToDiscard);
    take_turn(&mut game, "Bob clues 1 to Cathy");
    take_turn(&mut game, "Cathy clues yellow to Bob");

    // We should discard slot 4 (so that Bob plays the non-discarding dupe in slot 4).
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
}

#[test]
fn it_targets_an_unclued_dupe_for_dc() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y1", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            init: Box::new(|g| pre_clue(g, Bob, 4, &["red"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should play slot 4 (so that Bob discards slot 1).
    has_status(&game, Alice, 4, CardStatus::DragToPlay);
}

#[test]
fn it_targets_clued_trash_over_unclued_dupe_for_dc() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y1", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["yellow"]);
                pre_clue(g, Bob, 4, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should play slot 3 (so that Bob discards slot 2).
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
}

#[test]
fn it_targets_known_trash_over_cross_hand_dupe_for_dc() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y1", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "r4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            init: Box::new(|g| fully_known(g, Cathy, 5, "r4")),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");
    // We should play slot 3 (so that Bob discards slot 2).
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
    take_turn(&mut game, "Alice plays b1 (slot 3)");
    take_turn(&mut game, "Bob clues blue to Cathy");
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should target y1 again instead of r4.
    has_status(&game, Alice, 3, CardStatus::DragToPlay);
}

#[test]
fn it_reacts_to_a_sacrifice() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "b2", "y3", "p3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");

    // We should play slot 2 (Bob discards y3 in slot 3).
    has_status(&game, Alice, 2, CardStatus::DragToPlay);
}

#[test]
fn it_shifts_a_reaction() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "g2", "r2", "r3", "g5"],
            &["g1", "b5", "p2", "b1", "g4"],
        ],
        Setup {
            starting: Cathy,
            init: Box::new(|g| pre_clue(g, Alice, 3, &["5"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob");
    // Normally, Alice would play slot 3 -> Bob slot 1 = 4. However, slot 3 is a known 5.
    has_status(&game, Alice, 3, CardStatus::None);

    // Instead, Alice should play slot 1 -> Bob slot 3 as a finesse.
    has_status(&game, Alice, 1, CardStatus::DragToPlay);
    has_infs(&game, None, Alice, 1, &["r1"]);

    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays r1 (slot 1)");

    // Bob's slot 1 should still be allowed to be b1.
    assert!(
        game.common.thoughts[game.state.hands[Bob.ordinal()][0]]
            .inferred
            .contains(game.state.expand_short("b1").unwrap())
    );
}

#[test]
fn it_targets_a_self_connection() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r5", "b2", "b3", "g4"],
            &["g5", "g2", "r2", "b1", "y3"],
        ],
        Setup {
            clue_tokens: 7,
            starting: Cathy,
            init: Box::new(|g| pre_clue(g, Bob, 1, &["1"])),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob");

    // Alice's slot 4 is called to discard (4 + 3 = 2)
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
}

#[test]
fn it_stacks_on_top_of_its_own_self_connection() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r1", "y1", "y1", "g1"],
            &["r2", "g2", "b1", "b1", "g1"],
        ],
        Setup {
            clue_tokens: 4,
            starting: Bob,
            play_stacks: Some(vec![4, 4, 4, 2, 2]),
            init: Box::new(|g| fully_known(g, Alice, 5, "r5")),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues purple to Alice (slots 1,2)");
    take_turn_draw(&mut game, "Cathy discards r2", "b2"); // Alice's slot 1 is known p3
    take_turn(&mut game, "Alice plays r5 (slot 5)"); // slot 1 moves to slot 2

    take_turn(&mut game, "Bob clues green to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy discards b2", "p2"); // Alice's slot 3 is known p4

    has_infs(&game, None, Alice, 2, &["p3"]);
    has_infs(&game, None, Alice, 3, &["p4"]);

    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][1])
    );
}

#[test]
fn it_considers_discarding_kt_to_be_a_reaction() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "r5", "b2", "b3", "g4"],
            &["g5", "g2", "r2", "b1", "y3"],
        ],
        Setup {
            clue_tokens: 7,
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| fully_known(g, Alice, 4, "r1")),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Bob");
    // Alice's slot 4 is called to discard, even though she would already be discarding it.
    has_status(&game, Alice, 4, CardStatus::CalledToDiscard);
    take_turn(&mut game, "Alice discards g1 (slot 4)");

    has_status(&game, Bob, 1, CardStatus::DragToPlay);
}

#[test]
fn it_doesnt_assume_others_will_react_to_fix_when_they_dont_know() {
    let mut game = setup(
        reactor(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "g1", "y4", "g3", "r1"],
            &["r2", "g1", "b2", "r3", "r1"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    take_turn(&mut game, "Alice plays y1 (slot 5)");
    take_turn(&mut game, "Bob clues blue to Cathy");

    take_turn_draw(&mut game, "Cathy plays g1", "y1");
    take_turn(&mut game, "Alice clues 2 to Cathy");

    // Cathy will bomb g1 trying to play b1.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
