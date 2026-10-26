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
fn does_not_finesse_from_a_2_save() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r5", "r4", "r2", "y4", "y2"],
            &["g5", "b4", "g1", "y2", "b3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Bob");
    has_infs(&game, None, Bob, 5, &["r2", "y2", "g2", "b2", "p2"]);
    assert!(
        !game.common.thoughts[game.state.hands[Alice.ordinal()][0]]
            .inferred
            .is_empty()
    );
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn counts_playables_connecting_on_unknown_plays() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "g1", "p5", "r2", "y2"],
            &["g2", "g3", "p2", "p1", "b4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Bob");
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Alice clues 2 to Cathy");
    let (_, playables) = playables_result(&game, &hypo);
    assert_eq!(playables.len(), 1);
}

#[test]
fn counts_playables_fake_connecting_on_unknown_plays() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "g4", "g4", "y4", "y4"],
            &["p4", "b4", "b4", "r4", "r4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 4,5)");
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Cathy clues red to Bob");
    let (_, playables) = playables_result(&game, &hypo);
    assert_eq!(playables.len(), 1);
}

#[test]
fn doesn_t_clue_a_duplicate_of_a_commonly_unknown_play() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "g1", "y4", "y4"],
            &["r4", "g1", "b1", "y1"],
            &["g4", "g4", "b4", "b4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 1 to Cathy");
    let mut hypo = game.clone();
    take_turn(&mut hypo, "Alice clues 1 to Bob");
    let (_, playables) = playables_result(&game, &hypo);
    assert!(playables.is_empty());

    let action = ClueAction {
        giver: Alice.ordinal(),
        target: Bob.ordinal(),
        list: vec![game.state.hands[Bob.ordinal()][1]],
        clue: BaseClue {
            kind: ClueKind::Rank,
            value: 1,
        },
    };
    assert_eq!(get_result(&game, &hypo, &action, false), -100.0);
}

#[test]
fn connects_an_unknown_playable_on_self() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r4", "b4", "g4", "y3"],
            &["y1", "y2", "b4", "g4"],
            &["y4", "r4", "p4", "g2"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3,4)");
    take_turn(&mut game, "Cathy clues 2 to Donald");
    take_turn(&mut game, "Donald clues 2 to Cathy");
    has_infs(&game, None, Cathy, 2, &["r2", "y2", "g2", "b2", "p2"]);
    // Alice is promised to have y1, since Cathy will never self-finesse.
    let y1 = game.state.expand_short("y1").unwrap();
    assert!(game.common.links.iter().any(|l| matches!(l,
        Link::Promised { orders, id, .. } if *orders == FastBitSet::from([0, 1, 2]) && *id == y1)));
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
    take_turn(&mut game, "Alice clues yellow to Bob");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
    take_turn(&mut game, "Bob clues 2 to Donald");
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 1)");
    // Cathy demonstrated that we have y1.
    let cathy_slot1 = game.state.hands[Cathy.ordinal()][0];
    assert!(
        game.common.thoughts[cathy_slot1].inferred.len() > 5,
        "Expected nothing special, got {}",
        game.common.str_infs(&game.state, cathy_slot1)
    );
    has_status(&game, Cathy, 1, CardStatus::None);
}

#[test]
fn connects_an_unknown_playable_through_other() {
    let mut game = setup(
        hgroup(9),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "y2", "b4", "g4"],
            &["y4", "r4", "p4", "g2"],
            &["r2", "g1", "b1", "y1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Donald");
    take_turn(&mut game, "Bob clues 2 to Cathy");
    take_turn(&mut game, "Cathy clues 2 to Bob");
    let g = &game;
    has_infs(g, None, Bob, 2, &["y2", "g2", "b2"]);
    assert_eq!(g.last_move(), Some(Interp::Clue(ClueInterp::Play)));
    take_turn(&mut game, "Donald clues yellow to Alice (slot 4)");
    has_infs(&game, None, Alice, 4, &["y3"]);
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Play)));
}

#[test]
fn plays_the_focus_first() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "b1", "p1"],
            &["r1", "y1", "g1", "b1", "p1"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![3, 3, 3, 3, 3]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 4 to Alice (slots 2,4,5)");
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn gives_a_5_save_even_if_a_2_will_be_lost() {
    let game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g4", "g4", "b4", "y2", "r5"],
        ],
        Setup { ..Setup::default() },
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Rank {
            target: Bob.ordinal(),
            value: 5
        }
    );
}

#[test]
fn entertains_all_possible_connections() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "p4", "g4", "b3"],
            &["g4", "p5", "b1", "p1"],
            &["r4", "r4", "y4", "y5"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 1, 0, 0, 0]),
            discarded: &["b3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Cathy");
    take_turn(&mut game, "Cathy clues green to Bob");
    take_turn(&mut game, "Donald clues 2 to Alice (slot 3)");
    has_infs(&game, None, Alice, 3, &["y2", "g2", "b2", "p2"]);
    take_turn(&mut game, "Alice clues 5 to Donald");
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn_draw(&mut game, "Cathy plays p1", "r1");
    take_turn(&mut game, "Donald clues 3 to Alice (slot 1)"); // x3 xx x2 xx
    // Alice cannot play slot 1, since it might be y3 or p3.
    // She also can't play slot 3, since it might be b2.
    assert!(
        game.me()
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}

#[test]
fn doesn_t_create_a_play_link_for_an_unplayable_save_connection() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r5", "y3", "b4", "p3"],
            &["r1", "y1", "b4", "p3"],
            &["r2", "g3", "g3", "p4"],
        ],
        Setup {
            starting: Donald,
            discarded: &["b3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 1 to Cathy");
    take_turn(&mut game, "Alice clues 2 to Donald");
    take_turn(&mut game, "Bob clues 3 to Alice (slot 4)"); // could be r3 or b3
    take_turn_draw(&mut game, "Cathy plays r1", "y4");
    take_turn_draw(&mut game, "Donald plays r2", "y5");
    has_infs(&game, None, Alice, 4, &["r3", "b3"]);
}

#[test]
fn doesn_t_write_a_play_link_too_early_when_another_player_could_connect() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slot 2)");
    take_turn(&mut game, "Alice plays b1 (slot 5)");
    // Alice's slot 3 (previously slot 2) can still be r2.
    has_infs(&game, None, Alice, 3, &["r2", "b2"]);
}

#[test]
fn preserves_info_lock_against_good_touch() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g3", "b4", "p4"],
            &["r4", "y4", "g2", "b3", "r5"],
        ],
        Setup {
            play_stacks: Some(vec![0, 2, 2, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob"); // could be y3 or g3
    take_turn(&mut game, "Bob clues green to Alice (slot 4)"); // duping g3
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 5)"); // neg 5 on our gree card
    take_turn(&mut game, "Alice clues 5 to Cathy");
    take_turn_draw(&mut game, "Bob plays g3", "y1");
    let g = &game;
    // Our slot 5 should be trash.
    assert_eq!(
        g.common.thinks_trash(g, Alice.ordinal()),
        vec![g.state.hands[Alice.ordinal()][3]]
    );
    take_turn_draw(&mut game, "Cathy discards b3", "g1");
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][3]
        }
    );
}

#[test]
fn doesn_t_give_an_invalid_ambiguous_prompt() {
    let mut game = setup(
        hgroup(1),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "r1", "r3", "y3", "y1"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Bob, 3, &["red"]);
                pre_clue(g, Bob, 4, &["yellow"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 4 to Bob");
    // Bob will prompt r3 -> r4.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
