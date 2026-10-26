//! Tests of the model beyond what the ported Scala tests cover: hypo stacks, simulation, replay and rewind.

mod common;

use common::*;
use scala_bot::basics::Identity;
use scala_bot::basics::action::{Action, TurnAction};

fn two_player() -> scala_bot::basics::Game {
    setup(
        neutral(),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "b1", "y3", "g4", "p5"],
        ],
        Setup {
            starting: Player::Bob,
            ..Setup::default()
        },
    )
}

#[test]
fn a_fully_known_playable_advances_the_hypo_stacks() {
    let mut game = two_player();
    fully_known(&mut game, Player::Bob, 1, "r1");
    game.elim(None, false);

    assert_eq!(game.common.hypo_stacks, vec![1, 0, 0, 0, 0]);
    assert_eq!(game.common.hypo_plays.len(), 1);
    // Our own stacks don't change just because we see Bob's hand.
    assert_eq!(game.me().hypo_stacks[0], 1);
}

#[test]
fn clued_ones_are_unknown_plays() {
    let mut game = two_player();
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");

    assert_eq!(game.common.unknown_plays.len(), 2);
    assert_eq!(game.common.hypo_stacks, vec![0; 5]);
}

#[test]
fn simulating_an_action_leaves_the_game_untouched() {
    let game = two_player();
    let before_turn = game.state.turn_count;
    let order = game.state.hands[1][4];

    let action = Action::Discard(scala_bot::basics::action::DiscardAction {
        player_index: 1,
        order,
        id: game.state.deck[order].identity,
        failed: false,
    });
    let hypo = game.simulate_action(&action, Some(Identity::new(0, 2)), false);

    assert_eq!(game.state.turn_count, before_turn);
    assert_eq!(game.state.hands[1].len(), 5);
    assert_eq!(hypo.state.hands[1].len(), 5);
    assert!(!hypo.state.hands[1].contains(&order));
    assert_eq!(hypo.state.discard_stacks[4][4], vec![order]);
}

#[test]
fn replaying_reproduces_the_game() {
    let mut game = two_player();
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");
    take_turn_draw(&mut game, "Alice plays r1 (slot 2)", "");
    take_turn_draw(&mut game, "Bob discards y3", "g2");

    let replayed = game.replay(1).expect("replay failed");

    assert_eq!(replayed.state.turn_count, game.state.turn_count);
    assert_eq!(replayed.state.hands, game.state.hands);
    assert_eq!(replayed.state.play_stacks, game.state.play_stacks);
    assert_eq!(replayed.state.discard_stacks, game.state.discard_stacks);
    assert_eq!(replayed.state.clue_tokens, game.state.clue_tokens);
    for order in game.state.held_orders {
        assert_eq!(
            replayed.common.thoughts[order].possible,
            game.common.thoughts[order].possible
        );
    }
}

#[test]
fn rewinding_refuses_to_insert_the_same_action_twice() {
    let mut game = two_player();
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");

    let turn = game.state.turn_count - 1;
    let action = game.state.action_list[turn][0].clone();
    assert_eq!(
        game.rewind(turn, &action).unwrap_err(),
        "action was already rewinded"
    );
    assert!(
        game.rewind(0, &action)
            .unwrap_err()
            .contains("invalid turn")
    );
}

#[test]
fn rewinding_inserts_an_interp_action() {
    let mut game = two_player();
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");

    let interp = Action::Interp(scala_bot::basics::action::InterpAction {
        interp: scala_bot::basics::ClueInterp::Fix,
    });
    let turn = game.state.turn_count - 1;
    let rewound = game.rewind(turn, &interp).expect("rewind failed");

    assert_eq!(rewound.state.turn_count, game.state.turn_count);
    assert_eq!(
        rewound.next_interp,
        Some(scala_bot::basics::ClueInterp::Fix.into())
    );
}

#[test]
fn rewind_depth_is_limited() {
    let mut game = two_player();
    take_turn(&mut game, "Bob clues 1 to Alice (slots 2,3)");
    game.rewind_depth = 5;

    let action = Action::Turn(TurnAction {
        num: 99,
        current_player_index: Some(0),
    });
    assert_eq!(
        game.rewind(1, &action).unwrap_err(),
        "rewind depth went too deep"
    );
}
