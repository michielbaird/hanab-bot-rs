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
fn understands_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g1", "y1", "r2", "y3", "p3"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues yellow to Alice (slot 3)");
    let g = &game;
    has_infs(g, None, Alice, 3, &["y1", "y2"]);
    take_turn_draw(&mut game, "Cathy plays g1", "b1");
    take_turn(&mut game, "Alice discards b1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4", "r1");
    take_turn_draw(&mut game, "Cathy plays y1", "y1");
    // Alice's slot 4 (used to be 3) should just be y2 now.
    has_infs(&game, None, Alice, 4, &["y2"]);
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn understands_a_fake_asymmetric_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b2", "r1", "r4", "r3"],
            &["r5", "g2", "g3", "g1"],
            &["b5", "y4", "r4", "b2"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 0, 0, 1, 0]),
            init: Box::new(|g| {
                pre_clue(g, Cathy, 4, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 4 to Alice (slots 2,3)");
    let g = &game;
    has_infs(g, None, Alice, 2, &["r4", "g4", "b4"]);
    take_turn(&mut game, "Alice clues 2 to Cathy");
    take_turn_draw(&mut game, "Bob plays b2", "b1");
    take_turn_draw(&mut game, "Cathy plays g1", "y2");
    take_turn(&mut game, "Donald clues 3 to Cathy");
    // We should have b3 in slot 1.
    has_infs(&game, None, Alice, 1, &["b3"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn plays_into_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b5", "p4", "y2", "g3", "r3"],
            &["r4", "r4", "g4", "r5", "b4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob");
    let g = &game;
    has_infs(g, None, Alice, 1, &["y1"]);
    has_status(g, Alice, 1, CardStatus::Finessed);
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    // Alice's slot 2 should be [y1] now.
    has_infs(&game, None, Alice, 2, &["y1"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn understands_a_connecting_clue_on_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "r1", "b1", "r3"],
            &["y1", "y4", "g4", "b4"],
            &["r2", "r4", "g4", "p4"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Donald");
    take_turn(&mut game, "Donald clues yellow to Cathy");
    take_turn(&mut game, "Alice clues 3 to Bob");
    has_infs(&game, None, Bob, 4, &["r3", "g3", "b3", "p3"]);
}

#[test]
fn plays_into_a_complex_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b5", "p4", "y2", "g3"],
            &["g1", "r4", "g4", "y5"],
            &["g2", "r4", "b4", "g1"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Alice (slot 4)"); // g4
    take_turn_draw(&mut game, "Cathy plays g1", "p3");
    take_turn_draw(&mut game, "Donald plays g2", "r5");
    take_turn(&mut game, "Alice plays p1 (slot 1)");
    // Alice's slot 2 should be [g3] now.
    has_infs(&game, None, Alice, 2, &["g3"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn understands_when_it_dupes_a_card_in_its_own_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "b4", "g2", "r5"],
            &["g1", "b2", "y3", "r4"],
            &["p1", "r3", "r1", "g3"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 4 to Cathy"); // r2 (Cathy), r3 (Alice)
    take_turn(&mut game, "Alice clues green to Bob"); // reverse finesse for g1
    take_turn_draw(&mut game, "Bob plays r2", "b4");
    take_turn_draw(&mut game, "Cathy plays g1", "g4");
    take_turn(&mut game, "Donald clues 5 to Alice (slot 4)");
    take_turn(&mut game, "Alice bombs g1 (slot 1)"); // trying to play r3
    // Slot 2 should still be finessed as r3.
    has_infs(&game, None, Alice, 2, &["r3"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn doesn_t_give_a_layered_finesse_on_the_same_id_twice() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y1", "y1", "p1", "r5", "b4"],
            &["r2", "y4", "p2", "g3", "r3"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues purple to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn doesn_t_give_a_selfish_layered_finesse_on_the_same_id_twice() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y2", "y2", "y3", "r5", "b4"],
            &["r2", "y4", "p2", "g3", "r3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Alice (slot 2)");
    take_turn(&mut game, "Alice clues yellow to Cathy");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn gracefully_handles_a_non_matching_clue_revealing_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "b5", "r2", "y1", "p4"],
            &["r4", "g2", "g4", "r5", "b4"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["y4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob"); // r2 layered finesse
    take_turn(&mut game, "Alice plays b1 (slot 1)"); // expecting r1
    take_turn(&mut game, "Bob clues yellow to Alice (slots 2,5)"); // y4 save
    take_turn_draw(&mut game, "Cathy discards b4", "b1");
    let g = &game;
    // The revealed card should be known y1.
    has_infs(g, None, Alice, 2, &["y1"]);
    // Slot 3 should be finessed as the missing r1.
    has_infs(g, None, Alice, 3, &["r1"]);
    has_status(g, Alice, 3, CardStatus::Finessed);
    take_turn(&mut game, "Alice plays r1 (slot 3)");
    // y1 inference should remain (now on slot 3).
    has_infs(&game, None, Alice, 3, &["y1"]);
}

#[test]
fn gracefully_handles_a_matching_clue_revealing_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "b5", "r2", "y1", "p4"],
            &["y4", "g2", "g4", "r5", "b4"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["r4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob"); // r2 layered finesse
    take_turn(&mut game, "Alice plays b1 (slot 1)"); // expecting r1
    take_turn(&mut game, "Bob clues red to Alice (slots 3,5)"); // r4 save
    take_turn_draw(&mut game, "Cathy discards b4", "b1");
    // Slot 2 should be finessed as [y1, g1, b2, p1].
    has_infs(&game, None, Alice, 2, &["y1", "g1", "b2", "p1"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
    // Slot 3 should be known as the missing r1.
    has_infs(&game, None, Alice, 3, &["r1"]);
}

#[test]
fn gracefully_handles_a_complex_non_matching_clue_revealing_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "b5", "r4", "y1", "p4"],
            &["p3", "r2", "g4", "p5", "b4"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["y3"],
            init: Box::new(|g| {
                fully_known(g, Cathy, 2, "r2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob"); // r4 layered finesse (Alice plays r1, r3)
    take_turn(&mut game, "Alice plays b1 (slot 1)"); // expecting r1
    take_turn(&mut game, "Bob clues 3 to Alice (slots 1,4,5)"); // y3 save
    take_turn_draw(&mut game, "Cathy discards b4", "b1");
    let g = &game;
    // The revealed card should be known y3.
    has_infs(g, None, Alice, 4, &["r3"]);
    has_status(g, Alice, 2, CardStatus::Finessed);
    has_status(g, Alice, 3, CardStatus::Finessed);
    take_turn(&mut game, "Alice plays r1 (slot 2)");
    has_status(&game, Alice, 3, CardStatus::Finessed);
}

#[test]
fn plays_into_a_layered_finesse_with_self_connecting_cards() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b1", "b4", "y2", "r5", "r4"],
            &["g1", "r1", "b5", "g4", "b4"],
        ],
        Setup {
            starting: Cathy,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues yellow to Bob"); // y2 layered finesse
    take_turn(&mut game, "Alice plays p1 (slot 1)"); // expecting y1
    take_turn_draw(&mut game, "Bob discards r4", "b2");
    take_turn_draw(&mut game, "Cathy discards b4", "b3");
    take_turn(&mut game, "Alice plays p2 (slot 2)"); // expecting y1
    has_infs(&game, None, Alice, 3, &["y1"]);
}

#[test]
fn doesn_t_connect_on_others_inside_a_layered_finesse_on_self() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "b4", "y2", "r5"],
            &["p2", "y1", "b5", "g3"],
            &["g1", "r4", "b4", "g3"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues yellow to Bob"); // y2 layered finesse
    take_turn(&mut game, "Alice plays p1 (slot 1)"); // expecting y1
    // We should connect with y1 in slot 2, not using Bob's y1.
    has_infs(&game, None, Alice, 2, &["y1"]);
}

#[test]
fn doesn_t_accomodate_impossible_layers() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g2", "b2", "g4"],
            &["g5", "p2", "g1", "p1"],
            &["p4", "r4", "y5", "y3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Bob"); // finessing b1
    take_turn(&mut game, "Donald clues 4 to Alice (slot 2)");
    // Not enough cards to satisfy other possibilities.
    has_infs(&game, None, Alice, 2, &["g4", "b4"]);
}

#[test]
fn understands_when_a_fake_layered_finesse_is_disproved() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g3", "g2", "g4", "b4"],
            &["g5", "p2", "g2", "b3"],
            &["p4", "y3", "y5", "r2"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 0, 1, 0]),
            discarded: &["b3", "b4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Cathy"); // b3 save
    take_turn(&mut game, "Bob clues red to Donald"); // r1 reverse finesse
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues blue to Bob"); // b4 save (could be b2 play)
    let g = &game;
    has_infs(g, None, Bob, 4, &["b2", "b4"]);
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_infs(&game, None, Bob, 4, &["b2", "b4"]);
}

#[test]
fn allows_layered_players_to_unintentionally_dupe() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g3", "g2", "g4", "b2", "p4"],
            &["b2", "r2", "g2", "b3", "y3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 0, 1, 0]),
            init: Box::new(|g| {
                fully_known(g, Alice, 5, "r1");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Alice (slots 2,5)"); // r2 direct, or r3 layered
    take_turn(&mut game, "Cathy clues blue to Bob"); // dupe b2
    take_turn(&mut game, "Alice plays r1 (slot 5)");
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    take_turn_draw(&mut game, "Cathy plays b2", "p2");
    // r3 layered finesse in slot 3 (previously slot 2) is confirmed, not r2.
    has_infs(&game, None, Alice, 3, &["r3"]);
}

#[test]
fn doesn_t_allow_a_reveal_to_undo_a_previous_stall() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b5", "r1", "r1", "b1", "p2"],
            &["g4", "b1", "g1", "g1", "p1"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![5, 5, 3, 1, 1]),
            init: Box::new(|g| {
                fully_known(g, Bob, 5, "p2");
                fully_known(g, Cathy, 1, "g4");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Alice (slot 3)"); // b4 (self-double), p4 (selfish)
    take_turn_draw(&mut game, "Cathy plays g4", "y1");
    take_turn(&mut game, "Alice clues 5 to Bob"); // 5 Stall
    take_turn_draw(&mut game, "Bob plays p2", "y1");
    take_turn_draw(&mut game, "Cathy discards p1", "p1");
    take_turn(&mut game, "Alice plays b2 (slot 1)");
    // Bob's 5 can be [g5,b5,p5].
    // If Alice rewinded and thought she had a playable b2 when she 5 Stalled,
    // the 5 would be considered a play clue on [g5,b5].
    has_infs(&game, None, Bob, 2, &["g5", "b5", "p5"]);
}

#[test]
fn doesn_t_react_when_a_self_layered_finesse_is_demonstrated() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "b4", "b4", "p4"],
            &["r1", "g5", "g4", "y4", "p4"],
        ],
        Setup {
            starting: Bob,
            clue_tokens: 7,
            play_stacks: Some(vec![0, 0, 3, 0, 0]),
            init: Box::new(|g| {
                g.hg_mut().in_early_game = false;
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn_draw(&mut game, "Cathy plays r1", "r1");
    // Alice is not finessed.
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn shifts_into_a_layered_finesse_properly() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b3", "b2", "g2", "y5"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::WhiteOnesDarkPrism5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 4)"); // g1 self-finesse
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues yellow to Alice (slot 3)"); // y1 self-finesse
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_status(&game, Alice, 5, CardStatus::Finessed);
}

#[test]
fn handles_a_purge_properly() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b4", "b3", "b2", "g4", "y5"],
        ],
        Setup {
            starting: Bob,
            variant: TestVariant::WhiteOnesDarkPrism5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues green to Alice (slot 4)"); // g1 self-finesse
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues yellow to Alice (slot 3)"); // y1 self-finesse
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][0])
    );
    take_turn(&mut game, "Alice plays r1 (slot 1)");
    take_turn_draw(&mut game, "Bob discards g4", "y4");
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][1])
    );
    take_turn(&mut game, "Alice plays b1 (slot 2)");
    // It is now impossible to fulfill both the green and yellow finesses.
}

#[test]
fn interprets_a_prompt_after_a_fake_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g4", "y3", "y3"],
            &["y1", "r1", "b4", "r3"],
            &["p4", "p4", "b4", "y4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues red to Alice (slots 3,4)"); // could be r1 direct or r2 reverse
    take_turn(&mut game, "Alice clues green to Bob");
    take_turn(&mut game, "Bob clues 3 to Cathy"); // getting r3
    take_turn_draw(&mut game, "Cathy discards b4", "y1"); // proving not finesse
    // r1 is known to be in slot 4, with r2 prompted in slot 3.
    has_infs(&game, None, Alice, 4, &["r1"]);
    has_infs(&game, None, Alice, 3, &["r2"]);
}

#[test]
fn interprets_a_prompt_after_a_fake_finesse_2() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "y4", "g2", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p5"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Bob"); // reverse finesse on r2
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn(&mut game, "Cathy clues green to Bob"); // Alice attempts to insert g1 behind r1
    take_turn(&mut game, "Alice plays b1 (slot 2)"); // ... but she still doesn't play r1 or g1
    take_turn(&mut game, "Bob clues 5 to Alice (slots 4,5)");
    // It is now impossible to fit both r1 and g1 in the initial finesse. r1 must be in slot 3 and g1 must be in slot 2 (previously slot 1).
    has_infs(&game, None, Alice, 3, &["r1"]);
    has_infs(&game, None, Alice, 2, &["g1"]);
}
