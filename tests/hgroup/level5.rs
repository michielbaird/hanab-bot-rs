#![allow(unused_imports)]
use super::common::*;
use hanab_bot::basics::action::ClueAction;
use hanab_bot::basics::action::{Action, DiscardAction};
use hanab_bot::basics::clue::{BaseClue, ClueKind};
use hanab_bot::basics::clue_result::{bad_touch_result, playables_result};
use hanab_bot::basics::connection::Connection;
use hanab_bot::basics::interp::{ClueInterp, DiscardInterp, Interp, PlayInterp};
use hanab_bot::basics::player::Link;
use hanab_bot::basics::{CardStatus, Identity, PerformAction};
use hanab_bot::collections::FastBitSet;
use hanab_bot::endgame::EndgameSolver;
use hanab_bot::frac::Frac;
use hanab_bot::hgroup::state_eval::eval_action_impl;
use hanab_bot::hgroup::state_eval::get_result;
use hanab_bot::hgroup::{DcStatus, FStatus, HGroupGame, StallInterp};

use Player::*;

#[test]
fn understands_a_fake_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g1", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 2)");
    has_infs(&game, None, Alice, 2, &["g1", "g2"]);
    assert_eq!(
        game.meta[game.state.hands[Cathy.ordinal()][0]].reasoning,
        vec![1]
    );
    take_turn_draw(&mut game, "Cathy discards p3", "r1");
    has_infs(&game, None, Alice, 2, &["g1"]);
    assert!(
        game.meta[game.state.hands[Cathy.ordinal()][0]]
            .reasoning
            .is_empty()
    );
}

#[test]
fn doesn_t_eliminate_from_a_possibly_fake_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "y1", "p1", "r4"],
            &["g4", "r1", "r3", "r2"],
            &["b1", "b2", "p4", "p3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![5, 5, 5, 0, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Alice (slot 4)"); // b2, p2 save
    take_turn(&mut game, "Cathy clues blue to Bob");
    take_turn_draw(&mut game, "Donald plays b1", "b1");
    has_infs(&game, None, Alice, 4, &["b2", "p2"]);
    has_infs(&game, None, Bob, 1, &["b2", "b3"]);
}

#[test]
fn understands_a_self_connecting_play_clue() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g3", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Alice (slot 4)"); // b2, p2 save
    take_turn(&mut game, "Cathy clues 2 to Alice (slot 3)");
    take_turn(&mut game, "Alice plays g1 (slot 4)");
    // Note: slot 3 has moved to slot 4
    has_infs(&game, None, Alice, 4, &["g2"]);
}

#[test]
fn understands_a_delayed_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "r4", "g4", "r5", "b4"],
            &["r3", "b3", "r2", "y3", "p3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 1, 1, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Cathy"); // r2, g2, b2
    take_turn(&mut game, "Bob clues red to Alice (slot 3)");
    let g = &game;
    has_infs(g, None, Alice, 3, &["r3", "r4"]);
    take_turn_draw(&mut game, "Cathy plays r2", "y1");
    let g = &game;
    has_infs(g, None, Alice, 3, &["r3", "r4"]);
    take_turn(&mut game, "Alice discards b1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4", "r1");
    take_turn_draw(&mut game, "Cathy plays r3", "g1");
    // Note: slot 3 has moved to slot 4
    has_infs(&game, None, Alice, 4, &["r4"]);
}

#[test]
fn understands_a_fake_delayed_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p4", "r4", "g4", "r5", "b4"],
            &["r2", "b3", "r1", "y3", "p3"],
        ],
        Setup {
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");
    take_turn(&mut game, "Bob clues red to Alice (slot 3)");
    take_turn_draw(&mut game, "Cathy plays r1", "y1");
    take_turn(&mut game, "Alice discards b1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4", "r1");
    take_turn_draw(&mut game, "Cathy discards p3", "g1");
    // Note: slot 3 has moved to slot 4
    has_infs(&game, None, Alice, 4, &["r2"]);
}

#[test]
fn understands_that_a_self_finesse_may_not_be_ambiguous() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "p4", "r2", "r3", "g4"],
            &["p2", "p1", "b3", "y3", "b4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Cathy");
    let g = &game;
    has_status(g, Cathy, 2, CardStatus::Finessed);
    take_turn_draw(&mut game, "Cathy discards b4", "r4");
    // Alice can deduce that she has a playable card on finesse position, but shouldn't play it.
    has_status(&game, Alice, 1, CardStatus::None);
    // Cathy should still be finessed (slot 2 has moved to slot 3).
    has_status(&game, Cathy, 3, CardStatus::Finessed);
}

#[test]
fn still_finesses_if_cards_in_the_finesse_are_clued_as_long_as_they_weren_t_the_original_finesse_target()
 {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "b1", "g3", "r1"],
            &["p5", "g3", "p1", "b3"],
            &["p1", "b1", "r3", "g1"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues blue to Cathy"); // p1, b1 layer -> b2 on us
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 4)");
    take_turn_draw(&mut game, "Donald plays p1", "b4");
    take_turn(&mut game, "Alice clues 1 to Donald"); // getting g1, but touches b1
    take_turn(&mut game, "Bob clues blue to Donald"); // focusing b4, but filling in b1
    take_turn_draw(&mut game, "Cathy discards p1", "g4");
    take_turn_draw(&mut game, "Donald plays b1", "b5");
    // Alice's b2 in slot 1 should still be finessed.
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b2"]);
    has_infs(&game, None, Donald, 2, &["b4"]);
}

#[test]
fn cancels_finesse_connections_when_clued_elsewhere() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "y5", "g3", "y3"],
            &["p5", "g3", "p1", "b3"],
            &["p4", "y2", "r3", "g1"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 0, 2, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues yellow to Donald");
    take_turn(&mut game, "Bob clues 4 to Alice (slot 2)"); // Could be y4, b4
    take_turn(&mut game, "Cathy clues yellow to Bob"); // Focusing y3
    // Alice's slot 1 should be [r2, b3, p1] (not g1 since both g3s visible).
    has_status(&game, Alice, 1, CardStatus::Finessed);
    let y3 = game.state.expand_short("y3").unwrap();
    assert!(
        !game.common.thoughts[game.state.hands[Alice.ordinal()][0]]
            .inferred
            .contains(y3)
    );
    // hasInfs(game, None, Alice, 1, Vector("r2", "b3", "p1"))
}

#[test]
fn doesn_t_confirm_symmetric_finesses_after_a_stomped_play() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "p2", "b2", "p2"],
            &["y1", "g4", "g3", "y4"],
            &["y1", "g1", "y3", "r4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            play_stacks: Some(vec![0, 0, 3, 1, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Donald");
    take_turn_draw(&mut game, "Donald plays y1", "p1");
    take_turn(&mut game, "Alice discards g1 (slot 4)");
    take_turn(&mut game, "Bob clues 4 to Cathy"); // y2 finesse on us, y3 prompt, y4 (p4 symmetric)
    take_turn(&mut game, "Cathy clues 1 to Donald");
    take_turn_draw(&mut game, "Donald plays p1", "p1");
    // Alice's y2 should still be finessed
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["y2"]);
}

#[test]
fn eliminates_all_finesse_possibilities_when_a_player_doesn_t_play() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b5", "r4", "r1", "r1"],
            &["y4", "m4", "b4", "b2"],
            &["m3", "g1", "y1", "b1"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 7,
            play_stacks: Some(vec![0, 2, 0, 0, 1]),
            variant: TestVariant::Rainbow5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues green to Alice (slots 2,3,4)");
    take_turn(&mut game, "Alice plays m2 (slot 4)");
    take_turn_draw(&mut game, "Bob discards r1 (slot 4)", "b1");
    take_turn(&mut game, "Cathy clues blue to Donald");
    // Clue could be b1 finesse (Bob) -> b2 (Donald)
    assert!(
        game.waiting()
            .iter()
            .any(|wc| wc.inference == Identity::new(3, 2) && wc.target == Donald.ordinal())
    );
    take_turn(&mut game, "Donald clues 3 to Alice (slots 2,3)"); // b1 reverse + self composition, or y3 direct
    has_infs(&game, None, Alice, 2, &["y3", "b3"]);
    take_turn(&mut game, "Alice clues green to Donald");
    take_turn(&mut game, "Bob clues red to Cathy");
    // After Bob doesn't play, both b1 and y3 should be known.
    has_infs(&game, None, Alice, 2, &["y3"]);
    has_infs(&game, None, Donald, 4, &["b1"]);
}

#[test]
fn interprets_a_delayed_play_through_rainbow_identities() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "r1", "b2", "y3"],
            &["r2", "y4", "b3", "r3"],
            &["m4", "m2", "r4", "g1"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 0, 0, 1]),
            variant: TestVariant::Rainbow5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 2 to Donald"); // m2
    take_turn(&mut game, "Bob clues green to Donald"); // g1, could be m3
    take_turn(&mut game, "Cathy clues green to Bob"); // g2
    // Alice should not be finessed.
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn identifies_maybe_finessed_cards() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "y4", "g4", "b4"],
            &["r4", "y4", "g4", "b4"],
            &["r4", "g5", "p4", "p4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Alice (slot 2)");
    // Bob might be finessed for r1, but we aren't sure.
    assert!(
        game.xmeta(game.state.hands[Bob.ordinal()][0])
            .f_status
            .contains(&FStatus::PossiblyOn(game.state.our_player_index))
    );
}

#[test]
fn identifies_more_maybe_finessed_cards() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "y4", "r4", "b4"],
            &["r1", "r2", "g4", "b4"],
            &["g1", "g4", "p4", "p4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy");
    take_turn(&mut game, "Cathy clues green to Donald");
    take_turn(&mut game, "Donald clues 3 to Alice (slot 4)");
    // We might be finessed for y2, but we aren't sure.
    assert!(
        game.xmeta(game.state.hands[Alice.ordinal()][0])
            .f_status
            .contains(&FStatus::PossiblyOn(game.state.our_player_index))
    );
}

#[test]
fn recognizes_certainly_finessed_cards() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "r2", "g4", "b4"],
            &["r4", "y4", "g4", "b4"],
            &["r4", "g5", "p4", "p4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Alice (slots 1,2,3,4)");
    // Bob must be finessed for r1.
    assert!(game.is_definite(game.state.hands[Bob.ordinal()][0]));
    has_infs(&game, None, Alice, 4, &["r3"]);
}

#[test]
fn cancels_a_prompt_after_the_reacting_player_stalls() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p5", "y4", "p4", "p2", "g3"],
            &["b4", "y2", "y1", "b3", "r2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Cathy");
    take_turn(&mut game, "Cathy clues yellow to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays y1 (slot 5)");
    take_turn(&mut game, "Bob clues 3 to Alice (slots 1,5)"); // x3 xx xx xx y3
    let g = &game;
    has_status(g, Alice, 2, CardStatus::None);
    take_turn(&mut game, "Cathy clues 5 to Bob");
    // After Cathy stalls, Alice's slot 2 should be finessed.
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn can_give_a_finesse_while_finessed() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "g2", "g3", "p4"],
            &["p1", "b2", "p3", "y4"],
            &["p2", "y2", "b3", "r4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Cathy");
    take_turn(&mut game, "Alice clues purple to Bob");
    // Alice's b1 should still be finessed.
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_a_critical_save_while_finessed_when_other_potential_givers_are_finessed() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "p2", "p3", "g3"],
            &["p4", "y5", "b3", "p5"],
            &["b4", "y2", "p4", "r4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues purple to Donald"); // finessing p1, p2, p3
    take_turn(&mut game, "Donald clues blue to Cathy"); // finesses b1, b2 in our hand
    take_turn(&mut game, "Alice clues 5 to Cathy");
    // Alice's b1 should still be finessed.
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_a_finesse_on_top_of_an_in_progress_connection_on_other() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p4", "r3", "g3", "b3"],
            &["p1", "r3", "p1", "y2"],
            &["b2", "y3", "r4", "b5"],
        ],
        Setup {
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob"); // finessing b2 (Donald)
    take_turn(&mut game, "Bob clues 5 to Donald");
    take_turn(&mut game, "Cathy clues 5 to Donald"); // should finesse b4 (Alice)
    has_status(&game, Alice, 1, CardStatus::Finessed);
    has_infs(&game, None, Alice, 1, &["b4"]);
}

#[test]
fn understands_a_finesse_on_top_of_an_in_progress_connection_on_us() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p1", "r3", "p1", "y2"],
            &["b2", "y3", "r4", "b5"],
            &["y2", "b4", "b3", "g4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 1, 1, 1, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 3 to Alice (slots 2,3,4)"); // finessing b2 (Donald)
    take_turn(&mut game, "Alice clues 5 to Cathy");
    take_turn(&mut game, "Bob clues 5 to Cathy"); // should finesse b4 (Donald)
    has_status(&game, Donald, 1, CardStatus::Finessed);
    has_infs(&game, None, Donald, 2, &["b4"]);
}

#[test]
fn understands_a_layered_player_won_t_play_if_their_promised_card_is_unplayable() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p2", "p3", "g4", "g3"],
            &["y1", "p4", "b5", "r3"],
            &["b4", "y5", "p5", "r4"],
            &["y3", "r1", "r3", "g4"],
        ],
        Setup {
            starting: Emily,
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Emily clues purple to Donald"); // finesses p1 (Alice), p2 (Bob), p3 (Bob), y1 (Cathy), p4 (Cathy)
    take_turn(&mut game, "Alice clues 5 to Donald");
    // Alice doesn't need to save Donald's 5, since Cathy won't be able to play on her turn.
    assert!(!game.important_action(Alice.ordinal()));
}

#[test]
fn doesn_t_give_nonsensical_clues() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y3", "b4", "b4", "p4", "y4"],
            &["r3", "r4", "g4", "g4", "y4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Alice (slots 1,5)");
    take_turn(&mut game, "Cathy clues 3 to Bob");
    take_turn(&mut game, "Alice clues red to Cathy"); // a weird bug with occam's razor suggests this clue
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}
