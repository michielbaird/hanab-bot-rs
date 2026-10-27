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
fn understands_a_hidden_finesse_rank() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "r4", "g4", "r5", "b4"],
            &["g2", "b3", "p3", "y3", "r2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 1, 1, 0]),
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Cathy, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 3)");
    let g = &game;
    has_infs(g, None, Alice, 3, &["r3", "g3"]);
    take_turn_draw(&mut game, "Cathy plays r2", "r1");
    let g = &game;
    // Alice's slot 3 should still be [r3,g3] to allow for a hidden finesse.
    has_infs(g, None, Alice, 3, &["r3", "g3"]);
    take_turn(&mut game, "Alice discards b1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards b4", "r1");
    take_turn_draw(&mut game, "Cathy discards p3", "y1");
    // Alice's slot 4 (used to be 3) should just be r3 now.
    has_infs(&game, None, Alice, 4, &["r3"]);
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn understands_a_complicated_fake_hidden_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "p4", "r3", "y3", "r5"],
            &["g1", "y2", "g2", "b1", "g3"],
        ],
        Setup {
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Cathy, 4, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues purple to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slot 1)");
    let g = &game;
    has_infs(g, None, Alice, 1, &["r2", "g2", "b2", "p2"]);
    take_turn_draw(&mut game, "Cathy plays b1", "y5");
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 2 to Cathy"); // y2 reverse finesse
    let g = &game;
    has_infs(g, None, Alice, 2, &["y1"]);
    take_turn_draw(&mut game, "Cathy discards g3", "p3");
    // Cathy didn't play into the green finesse, so we have [r2,b2,p2] in slot 1.
    has_infs(&game, None, Alice, 1, &["r2", "b2", "p2"]);
}

#[test]
fn plays_into_a_hidden_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g2", "r2", "r3", "p1", "b4"],
            &["p2", "g4", "y2", "b4", "p5"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 2,3)");
    take_turn(&mut game, "Alice plays y1 (slot 3)");
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn(&mut game, "Cathy clues red to Bob"); // r2 hidden finesse
    take_turn(&mut game, "Alice plays b1 (slot 3)"); // expecting r1 playable
    // Our slot 2 (previously slot 1) should be r1.
    has_infs(&game, None, Alice, 2, &["r1"]);
}

#[test]
fn generates_focus_possibilities_for_a_potential_hidden_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "y4", "b1", "r5", "g4"],
            &["y3", "g2", "b3", "p5", "p4"],
        ],
        Setup {
            starting: Cathy,
            init: Box::new(|g| {
                pre_clue(g, Bob, 3, &["1"]);
                fully_known(g, Cathy, 2, "g2");
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues green to Alice (slot 2)");
    has_infs(&game, None, Alice, 2, &["g1", "g3"]);
}

#[test]
fn realizes_a_hidden_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "r2", "y4", "g2"],
            &["y4", "g4", "p3", "y3"],
            &["p3", "g4", "g2", "b2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 3, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Cathy"); // looks like y1 (playable) -> y2 finesse on Alice
    take_turn_draw(&mut game, "Cathy discards g4", "p4");
    take_turn(&mut game, "Donald clues green to Bob"); // we need to play g1, but "y2" is in slot 1.
    let g = &game;
    // We are promised y1, y2 and g1
    // hasInfs(g, None, Alice, 3, Vector("y1", "g1"))
    has_status(g, Alice, 1, CardStatus::Finessed);
    // hasStatus(g, Alice, 2, CardStatus.Finessed)
    // Slot 2 is not playable.
    assert!(
        !g.common
            .thinks_playables(g, Alice.ordinal(), false, true)
            .contains(&g.state.hands[Alice.ordinal()][1])
    );
    take_turn(&mut game, "Alice plays y1 (slot 3)");
    // Alice's slot 2 (previously slot 1) should still be finessed.
    // hasInfs(game, None, Alice, 2, Vector("g1", "g3"))
    has_status(&game, Alice, 2, CardStatus::Finessed);
    // hasStatus(game, Alice, 3, CardStatus.Finessed)
}

#[test]
fn realizes_a_layered_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "r2", "y4", "g2"],
            &["p4", "g4", "p3", "y4"],
            &["p3", "g4", "g2", "b2"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 2, 0, 0, 0]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 3, &["1"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 1 to Alice (slot 4)");
    take_turn(&mut game, "Alice plays r1 (slot 4)");
    take_turn(&mut game, "Bob clues yellow to Cathy"); // Looks like y3 finesse from us
    take_turn(&mut game, "Cathy clues green to Bob"); // we need to play g1, but slot 1 is [y3] and slot 2 is neg 1.
    let g = &game;
    // hasInfs(g, None, Alice, 3, Vector("y1", "g1"))
    has_status(g, Alice, 1, CardStatus::Finessed);
    // hasStatus(g, Alice, 2, CardStatus.Finessed)
    // Slot 2 is not playable.
    assert!(
        !g.common
            .thinks_playables(g, Alice.ordinal(), false, true)
            .contains(&g.state.hands[Alice.ordinal()][1])
    );
    take_turn_draw(&mut game, "Donald discards b2", "b5");
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    // Alice's slot 2 should still be finessed.
    has_infs(&game, None, Alice, 2, &["y3"]);
    has_status(&game, Alice, 2, CardStatus::Finessed);
}

#[test]
fn correctly_resolves_an_ambiguous_hidden_finesse() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "y1", "g1", "g3"],
            &["b3", "y1", "g1", "b1"],
            &["p1", "p1", "r2", "y2"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![5, 5, 1, 1, 5]),
            init: Box::new(|g| {
                pre_clue(g, Alice, 2, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Bob"); // delayed play or hidden finesse
    take_turn(&mut game, "Donald clues 4 to Alice (slot 4)");
    // .tap: g =>
    //     // Can connect as b4 (if g2 hidden, then Cathy finesse) or g4
    //     hasInfs(g, None, Alice, 4, Vector("g4", "b4"))
    take_turn(&mut game, "Alice plays b2 (slot 2)"); // revealing hidden finesse
    let g = &game;
    // Slot 2 (previously slot 1) should be g2
    has_infs(g, None, Alice, 2, &["g2"]);
    take_turn_draw(&mut game, "Bob discards g1", "p2");
    take_turn_draw(&mut game, "Cathy plays b3", "r3"); // proving b4
    has_infs(&game, None, Alice, 4, &["b4"]);
}
