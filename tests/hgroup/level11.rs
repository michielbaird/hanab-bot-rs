#![allow(unused_imports)]
use super::common::*;
use scala_bot::basics::action::ClueAction;
use scala_bot::basics::action::{Action, DiscardAction};
use scala_bot::basics::clue::{BaseClue, ClueKind};
use scala_bot::basics::clue_result::{bad_touch_result, playables_result};
use scala_bot::basics::connection::Connection;
use scala_bot::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use scala_bot::basics::player::Link;
use scala_bot::basics::{CardStatus, Identity, PerformAction};
use scala_bot::collections::FastBitSet;
use scala_bot::endgame::EndgameSolver;
use scala_bot::frac::Frac;
use scala_bot::hgroup::state_eval::eval_action_impl;
use scala_bot::hgroup::state_eval::get_result;
use scala_bot::hgroup::{DcStatus, FStatus, HGroupGame, StallInterp};

use Player::*;

#[test]
fn understands_a_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "r1", "b1", "g5", "p2"],
            &["p1", "r4", "b5", "b2", "y4"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Cathy");
    let g = &game;
    has_status(g, Bob, 1, CardStatus::Bluffed);
    take_turn_draw(&mut game, "Bob plays b3", "y5");
    has_infs(&game, None, Cathy, 2, &["r4"]);
}

#[test]
fn understands_a_bluff_even_if_duplicating() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g3", "r2", "b4", "b2"],
            &["y1", "r4", "p4", "r4"],
            &["b2", "g1", "p3", "r3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn(&mut game, "Bob clues red to Donald");
    let g = &game;
    // hasStatus(g, Cathy, 1, CardStatus.MaybeBluffed)
    has_status(g, Cathy, 1, CardStatus::Bluffed);
    take_turn_draw(&mut game, "Cathy plays y1", "p5");
    has_infs(&game, None, Donald, 4, &["r3"]);
}

#[test]
fn plays_into_a_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5", "p2"],
            &["p1", "r4", "b5", "b2", "y4"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 2, 2, 2, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    let g = &game;
    // hasStatus(g, Alice, 1, CardStatus.FMaybeBluffed)
    has_status(g, Alice, 1, CardStatus::Bluffed);
    // hasInfs(g, None, Alice, 1, Vector("b3"))
    assert_eq!(
        g.take_action(),
        PerformAction::Play {
            target: g.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays r3 (slot 1)");
    has_infs(&game, None, Bob, 1, &["b4"]);
}

#[test]
fn plays_into_a_known_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g2", "b2"],
            &["p4", "b1", "b1", "b1", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob");
    let g = &game;
    has_status(g, Alice, 1, CardStatus::Bluffed);
    has_infs(g, None, Alice, 1, &["r1", "y1", "g1", "b1", "p1"]);
    assert_eq!(
        g.take_action(),
        PerformAction::Play {
            target: g.state.hands[Alice.ordinal()][0]
        }
    );
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_infs(&game, None, Bob, 5, &["b2"]);
}

#[test]
fn receives_a_simple_bluff_with_rank_disconnect() {
    let mut game = setup(
        hgroup(11),
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
    has_infs(&game, None, Alice, 2, &["r3", "g3"]);
}

#[test]
fn receives_a_bluff_with_rank_disconnect_in_pink() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "y2", "g2", "g2", "b2"],
            &["i1", "b4", "r4", "g3", "y3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            variant: TestVariant::Pink5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 3)");
    take_turn_draw(&mut game, "Cathy plays i1", "b5");
    has_infs(&game, None, Alice, 3, &["r3"]);
}

#[test]
fn doesn_t_perform_a_certain_discard_when_possibly_bluffed() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "r4", "g2", "p5"],
            &["r2", "g4", "b4", "b4"],
            &["g1", "r2", "y4", "y4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues red to Cathy"); // finessing r1
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "p4");
    take_turn_draw(&mut game, "Cathy discards r2", "y5"); // layered gd on Donald
    take_turn(&mut game, "Donald clues green to Bob");
    // hasStatus(game, Alice, 1, CardStatus.FMaybeBluffed)
    has_status(&game, Alice, 1, CardStatus::Bluffed);
    assert!(
        !game
            .find_discardable(Alice.ordinal())
            .contains(&game.state.hands[Alice.ordinal()][0])
    );
}

#[test]
fn doesn_t_allow_a_baton_target_to_be_a_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y3", "y4", "p4", "p4"],
            &["r2", "y4", "g4", "b4"],
            &["r4", "r1", "b4", "g4"],
        ],
        Setup {
            starting: Alice,
            clue_tokens: 5,
            init: Box::new(|g| {
                fully_known(g, Alice, 3, "r2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice discards r2 (slot 3)"); // baton to Cathy
    take_turn(&mut game, "Bob clues 1 to Donald");
    take_turn(&mut game, "Cathy clues blue to Alice (slot 4)");
    take_turn_draw(&mut game, "Donald plays r1", "b3");
    take_turn(&mut game, "Alice plays b1 (slot 4)");
    take_turn(&mut game, "Bob clues blue to Donald"); // finessing Alice for b2
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b2"]);
    take_turn_draw(&mut game, "Cathy plays r2", "p3");
    take_turn(&mut game, "Donald clues 5 to Alice (slot 4)");
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b2"]);
    assert_eq!(
        game.me()
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn infers_the_identity_of_indirect_bluffs() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5", "g3"],
            &["p1", "r4", "b5", "b2", "y3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 4)");
    take_turn_draw(&mut game, "Cathy plays p1", "p2");
    let g = &game;
    has_infs(g, None, Alice, 4, &["r2"]);
    take_turn(&mut game, "Alice discards g4 (slot 5)");
    take_turn(&mut game, "Bob clues red to Alice (slots 1,5)");
    take_turn_draw(&mut game, "Cathy plays p2", "p3");
    has_infs(&game, None, Alice, 1, &["r3"]);
}

#[test]
fn infers_the_identity_of_bluffed_connections_through_self() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5"],
            &["p1", "r4", "b5", "b2"],
            &["g4", "r4", "r5", "g3"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues red to Alice (slots 3,4)");
    take_turn(&mut game, "Alice plays r1 (slot 4)");
    take_turn(&mut game, "Bob clues red to Donald");
    take_turn_draw(&mut game, "Cathy plays p1", "g1");
    has_infs(&game, None, Alice, 4, &["r3"]);
}

#[test]
fn infers_the_identity_of_bluffed_connections_through_others() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5"],
            &["p1", "r4", "b5", "b2"],
            &["g4", "b2", "r3", "r1"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Donald");
    take_turn_draw(&mut game, "Donald plays r1", "p5");
    take_turn(&mut game, "Alice discards y4 (slot 4)");
    take_turn(&mut game, "Bob clues red to Alice (slots 2,3)");
    take_turn_draw(&mut game, "Cathy plays p1", "p2");
    has_infs(&game, None, Alice, 2, &["r4"]);
    has_infs(&game, None, Donald, 4, &["r3"]);
}

#[test]
fn connects_on_clued_cards_not_in_prompt_position() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "r1", "y1", "g5"],
            &["p1", "r4", "b5", "b2"],
            &["g4", "r5", "r3", "r1"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Donald");
    take_turn_draw(&mut game, "Donald plays r1", "p5");
    take_turn(&mut game, "Alice discards y4 (slot 4)");
    take_turn(&mut game, "Bob clues red to Alice (slots 2,3)");
    take_turn_draw(&mut game, "Cathy plays p1", "p2");
    has_infs(&game, None, Alice, 2, &["r4"]);
    let r3 = game.state.expand_short("r3").unwrap();
    assert!(game.common.links.iter().any(|l| matches!(l,
        Link::Promised { orders, id, .. } if *id == r3 && orders.len() == 2 && orders.contains(13) && orders.contains(14))));
}

#[test]
fn doesn_t_connect_if_unncessary() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r3", "r1", "y1", "r4"],
            &["b4", "r4", "b1", "b3"],
            &["g1", "r5", "r3", "r1"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 1, 0, 0, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Bob");
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    let g = &game;
    has_infs(g, None, Bob, 1, &["r3", "y3"]);
    take_turn(&mut game, "Bob clues 5 to Donald");
    take_turn(&mut game, "Cathy clues 4 to Bob");
    take_turn_draw(&mut game, "Donald plays g1", "y3");
    // p4 is a valid bluff target, without requiring the 3 to be red.
    has_infs(&game, None, Bob, 1, &["r3", "y3"]);
}

#[test]
fn prefers_a_bluff_connection_when_posssible() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "r1", "r1", "y1", "y1"],
            &["r2", "b4", "b4", "p4", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 2, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 4, &["red"]);
                pre_clue(g, Alice, 5, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Alice (slot 1)");
    take_turn_draw(&mut game, "Cathy plays r2", "y5");
    let g = &game;
    // Alice's 4 can be red or yellow.
    has_infs(g, None, Alice, 1, &["r4", "y4"]);
    take_turn(&mut game, "Alice discards y2 (slot 3)");
    // Alice's 4 can still be red or yellow.
    has_infs(&game, None, Alice, 2, &["r4", "y4"]);
}

#[test]
fn writes_the_correct_inferences_after_receiving_a_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p2", "y2", "g3", "b1"],
            &["r4", "b1", "g5", "r2"],
            &["p2", "r4", "r1", "b3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 1, 0, 0, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 1 to Bob");
    take_turn(&mut game, "Alice clues yellow to Bob");
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 2)");
    take_turn_draw(&mut game, "Donald plays p2", "b2");
    // If the 2 was blue, Donald wouldn't have played.
    has_infs(&game, None, Alice, 2, &["g2"]);
}

#[test]
fn plays_over_saving_a_playable() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p3", "r4", "p1", "b2", "y4"],
            &["y2", "y3", "y5", "p1", "g4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 1, 0, 0, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues yellow to Cathy");
    take_turn(&mut game, "Cathy clues blue to Bob");
    // Alice should play instead of saving y4.
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn understands_a_bluff_isn_t_deferred_by_another_bluff() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "p2", "b5", "b3"],
            &["y1", "g2", "p5", "b2"],
            &["g1", "y5", "b1", "g5"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 2)"); // bluff on Cathy?
    take_turn(&mut game, "Cathy clues purple to Bob"); // demonstrates not bluff
    has_infs(&game, None, Alice, 2, &["r1"]);
    // hasStatus(game, Donald, 1, CardStatus.MaybeBluffed)
    has_status(&game, Donald, 1, CardStatus::Bluffed);
}

#[test]
fn understands_a_bluff_isn_t_deferred_by_a_finesse() {
    let mut game = setup(
        hgroup(11),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "p2", "b5", "b3"],
            &["y1", "g2", "p5", "b2"],
            &["p1", "y5", "b1", "g5"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slot 2)"); // bluff on Cathy?
    take_turn(&mut game, "Cathy clues purple to Bob"); // demonstrates not bluff
    has_infs(&game, None, Alice, 2, &["r1"]);
    has_status(&game, Donald, 1, CardStatus::Finessed);
}
