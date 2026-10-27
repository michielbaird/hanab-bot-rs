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
fn understands_a_gentleman_s_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y4", "g4", "b4", "b2"],
            &["r1", "y4", "r4", "y3"],
            &["y5", "r5", "b1", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Donald"); // getting b1
    take_turn(&mut game, "Bob clues red to Cathy"); // getting r1
    take_turn_draw(&mut game, "Cathy plays r1", "b1");
    take_turn_draw(&mut game, "Donald discards b1", "b3");
    // Donald performed a Gentleman's Discard on Cathy.
    has_infs(&game, None, Cathy, 1, &["b1"]);
    has_status(&game, Cathy, 1, CardStatus::GentlemansDiscard);
}

#[test]
fn understands_a_gentleman_s_discard_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "g4", "b4", "b2"],
            &["r1", "y4", "r4", "y3"],
            &["y5", "r5", "b1", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues yellow to Bob"); // getting y1
    take_turn_draw(&mut game, "Bob discards y1", "y3");
    // Bob performed a Gentleman's Discard on us.
    has_infs(&game, None, Alice, 1, &["y1"]);
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .contains(&game.state.hands[Alice.ordinal()][0])
    );
    has_status(&game, Alice, 1, CardStatus::GentlemansDiscard);
}

#[test]
fn understands_a_layered_gentleman_s_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y4", "g4", "b4", "b2"],
            &["r1", "y2", "b1", "y3"],
            &["y5", "r5", "b1", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues blue to Donald"); // getting b1
    take_turn(&mut game, "Bob clues red to Cathy"); // getting r1
    take_turn_draw(&mut game, "Cathy plays r1", "y1");
    take_turn_draw(&mut game, "Donald discards b1", "b3");
    // Donald performed a Layered Gentleman's Discard on Cathy.
    has_status(&game, Cathy, 1, CardStatus::GentlemansDiscard);
    take_turn(&mut game, "Alice clues 2 to Bob");
    take_turn(&mut game, "Bob clues 5 to Donald");
    take_turn_draw(&mut game, "Cathy plays y1", "p4");
    has_status(&game, Cathy, 2, CardStatus::GentlemansDiscard);
}

#[test]
fn understands_a_layered_gentleman_s_discard_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "g4", "b4", "b2"],
            &["r1", "y4", "r4", "y5"],
            &["y3", "r5", "b1", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues yellow to Bob"); // getting y1
    take_turn_draw(&mut game, "Bob discards y1", "y3");
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues 5 to Cathy");
    // Donald performed a Gentleman's Discard on Alice.
    has_status(&game, Alice, 1, CardStatus::GentlemansDiscard);
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    // The layer is revealed.
    has_status(&game, Alice, 2, CardStatus::GentlemansDiscard);
}

#[test]
fn understands_a_revealed_layered_gd_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y4", "g4", "b4", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            clue_tokens: 4,
            init: Box::new(|g| {
                fully_known(g, Bob, 1, "r1");
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards r1", "r4");
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    take_turn(&mut game, "Bob clues yellow to Alice (slots 2,5)"); // focusing y3 on chop
    has_infs(&game, None, Alice, 2, &["y2"]);
    has_infs(&game, None, Alice, 3, &["r1"]);
    has_infs(&game, None, Alice, 5, &["y3"]);
}

#[test]
fn performs_a_gentleman_s_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "g4", "y2", "b4"],
            &["r5", "y4", "r4", "y3"],
            &["b1", "r4", "b4", "g3"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 7,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Alice (slots 3,4)"); // b1 in slot 4
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice discards b1 (slot 4)");
    // We performed a GD on Donald.
    has_status(&game, Donald, 1, CardStatus::GentlemansDiscard);
}

#[test]
fn performs_a_layered_gentleman_s_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "g4", "y2", "b4"],
            &["g2", "y4", "r4", "y3"],
            &["r1", "b1", "b4", "g3"],
        ],
        Setup {
            starting: Donald,
            clue_tokens: 6,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Alice (slots 3,4)"); // b1 in slot 4
    let g = &game;
    assert_eq!(
        g.take_action(),
        PerformAction::Discard {
            target: g.state.hands[Alice.ordinal()][3]
        }
    );
    take_turn(&mut game, "Alice discards b1 (slot 4)");
    // We performed a GD on Donald.
    has_status(&game, Donald, 1, CardStatus::GentlemansDiscard);
}

#[test]
fn understands_a_gd_with_finesse_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "g4", "b4", "b2"],
            &["r1", "y4", "r4", "y5"],
            &["y3", "r5", "b1", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues yellow to Bob"); // getting y1
    take_turn_draw(&mut game, "Bob discards y1", "y3");
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues blue to Bob"); // finessing b1
    let g = &game;
    // Alice should play slot 1.
    let slot1 = g.state.hands[Alice.ordinal()][0];
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![slot1]
    );
    assert_eq!(g.take_action(), PerformAction::Play { target: slot1 });
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    // Bob performed a gd on us. b1 is now expected to be in slot 2.
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Alice, 2, &["b1"]);
    // The finesse is still on (allow b34 symmetrically?).
    has_infs(&game, None, Bob, 4, &["b2"]);
}

#[test]
fn understands_a_layered_gd_with_finesse_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "g4", "b4", "b2"],
            &["r1", "y4", "r4", "y5"],
            &["y3", "r5", "b1", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues yellow to Bob"); // getting y1
    take_turn_draw(&mut game, "Bob discards y1", "y3");
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn(&mut game, "Donald clues blue to Bob"); // finessing b1
    let g = &game;
    // Alice should play slot 1.
    let slot1 = g.state.hands[Alice.ordinal()][0];
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![slot1]
    );
    assert_eq!(g.take_action(), PerformAction::Play { target: slot1 });
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    // Bob performed a layered gd on us. y1 is now expected to be in slot 2.
    has_status(&game, Alice, 2, CardStatus::GentlemansDiscard);
    has_infs(&game, None, Alice, 2, &["y1"]);
}

#[test]
fn understands_a_layered_finesse_with_gd_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "g4", "b4", "b4"],
            &["y1", "y4", "r4", "y5"],
            &["y3", "r5", "b2", "g5"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues yellow to Cathy"); // getting y1
    take_turn(&mut game, "Bob clues blue to Donald"); // reverse finesse on b2
    take_turn_draw(&mut game, "Cathy discards y1", "y3");
    take_turn(&mut game, "Donald clues 5 to Cathy");
    let g = &game;
    // Alice should play slot 1.
    let slot1 = g.state.hands[Alice.ordinal()][0];
    assert_eq!(
        g.common.thinks_playables(g, Alice.ordinal(), false, true),
        vec![slot1]
    );
    assert_eq!(g.take_action(), PerformAction::Play { target: slot1 });
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    // Bob performed a layered finesse on us. b1 is now expected to be in slot 2.
    has_status(&game, Alice, 2, CardStatus::Finessed);
    has_infs(&game, None, Alice, 2, &["b1"]);
    // The finesse is still on (allow b34 symmetrically?).
    has_infs(&game, None, Donald, 3, &["b2"]);
}

#[test]
fn understands_a_sarcastic_into_a_layered_finesse_not_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y1", "r1", "b4", "b4"],
            &["g1", "r2", "r3", "y5"],
            &["g1", "r4", "r5", "p3"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 3 to Cathy");
    take_turn_draw(&mut game, "Bob plays y1", "r4");
    take_turn(&mut game, "Cathy clues green to Donald");
    take_turn_draw(&mut game, "Donald discards g1", "p4"); // passing g1 back to Cathy
    // This isn't a GD on ALice.
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn understands_a_delayed_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y4", "r3", "y4", "g4", "g4"],
            &["r1", "b4", "b4", "p4", "r2"],
        ],
        Setup {
            init: Box::new(|g| {
                pre_clue(g, Cathy, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues 3 to Bob");
    take_turn(&mut game, "Bob clues green to Alice (slot 5)");
    take_turn_draw(&mut game, "Cathy plays r1", "y5");
    take_turn(&mut game, "Alice plays g1 (slot 5)");
    take_turn_draw(&mut game, "Bob discards r3", "p5");
    let g = &game;
    // GD for r3 in Alice's slot 1.
    has_infs(g, None, Alice, 1, &["r3"]);
    has_status(g, Alice, 1, CardStatus::GentlemansDiscard);
    take_turn_draw(&mut game, "Cathy plays r2", "b5");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    // Layered GD revealed, r3 moved to slot 2.
    has_infs(&game, None, Alice, 2, &["r3"]);
    has_status(&game, Alice, 2, CardStatus::GentlemansDiscard);
}

#[test]
fn doesn_t_perform_an_illegal_gentleman_s_discard() {
    let game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "g4", "r4", "p2", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![2, 2, 2, 1, 0]),
            clue_tokens: 7,
            init: Box::new(|g| {
                pre_clue(g, Alice, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    let action = Action::Discard(DiscardAction {
        player_index: Alice.ordinal(),
        order: game.state.hands[Alice.ordinal()][4],
        id: None,
        failed: false,
    });
    assert!(eval_action_impl(&game, &action) < 0.0);
}

#[test]
fn interprets_a_tcm_with_info_from_a_layered_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 1, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob discards r1", "g4");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2)");
    // This is a Trash CM on slot 1, with r1 in slot 2.
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][1]]
    );
    assert_eq!(
        game.take_action(),
        PerformAction::Play {
            target: game.state.hands[Alice.ordinal()][1]
        }
    );
    has_status(&game, Alice, 3, CardStatus::ChopMoved);
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Discard)));
}

#[test]
fn doesn_t_ocm_with_info_from_a_layered_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 0, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob discards r1", "g4");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2)");
    let g = &game;
    // r1 must be in slot 2, so g1 must be in slot 1: playing slot 1 is not an OCM.
    has_infs(g, None, Alice, 1, &["g1"]);
    has_infs(g, None, Alice, 2, &["r1"]);
    take_turn(&mut game, "Alice plays g1 (slot 1)");
    has_status(&game, Bob, 5, CardStatus::PermissionToDiscard);
}

#[test]
fn allows_an_ocm_with_ambiguous_info_from_a_layered_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "y4", "y4", "g4"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob discards r1", "g4");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2,3)");
    take_turn(&mut game, "Alice plays p1 (slot 3)");
    // Alice doesn't know where r1 is, and slot 1 can be a good 1, so playing slot 3 is a single OCM.
    has_status(&game, Bob, 4, CardStatus::PermissionToDiscard);
    has_status(&game, Bob, 5, CardStatus::ChopMoved);
}

#[test]
fn disambiguates_without_ocming_with_ambiguous_info_from_a_layered_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "y4", "y4", "g4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 0, 0, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn_draw(&mut game, "Bob discards r1", "g4");
    take_turn(&mut game, "Alice plays y1 (slot 1)");
    take_turn(&mut game, "Bob clues 1 to Alice (slots 1,2,3)");
    // Alice doesn't know where r1 is, and slot 1 might not be a good 1, so Alice should always play slot 2.
    // e.g. slots 1,2,3 could be [trash, g1, r1] or [g1, r1, trash]
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][1]]
    );
    let mut game1 = game.clone();
    take_turn(&mut game1, "Alice plays g1 (slot 2)");
    has_infs(&game1, None, Alice, 3, &["r1"]);
    has_status(&game1, Bob, 5, CardStatus::PermissionToDiscard);
    let mut game2 = game.clone();
    take_turn(&mut game2, "Alice plays r1 (slot 2)");
    has_infs(&game2, None, Alice, 2, &["g1"]);
    has_status(&game2, Bob, 5, CardStatus::PermissionToDiscard);
}

#[test]
fn correctly_interprets_a_play_clue_after_gd() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "g3", "g3", "p4", "p4"],
            &["y4", "y4", "b4", "b4", "g4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![3, 0, 0, 0, 0]),
            clue_tokens: 4,
            init: Box::new(|g| {
                fully_known(g, Bob, 1, "r4");
            }),
            ..Setup::default()
        },
    );
    take_turn_draw(&mut game, "Bob discards r4", "y3");
    take_turn(&mut game, "Cathy clues red to Alice (slots 1,2,5)");
    has_infs(&game, None, Alice, 1, &["r4"]);
    has_infs(&game, None, Alice, 5, &["r5"]);
    assert_eq!(
        game.common.thinks_trash(&game, Alice.ordinal()),
        vec![game.state.hands[Alice.ordinal()][1]]
    );
    assert_eq!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true),
        vec![game.state.hands[Alice.ordinal()][0]]
    );
}

#[test]
fn understands_a_baton_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b4", "g4", "y2", "b2"],
            &["r1", "y4", "p4", "y3"],
            &["y1", "r4", "b4", "b1"],
        ],
        Setup {
            discarded: &["r4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues blue to Donald"); // getting b1
    take_turn(&mut game, "Bob clues 4 to Donald"); // saving r4
    take_turn(&mut game, "Cathy clues yellow to Donald"); // getting y1
    take_turn_draw(&mut game, "Donald discards b4", "b3");
    // Donald performed a Baton Discard on Bob.
    has_infs(&game, None, Bob, 1, &["b4"]);
    has_status(&game, Bob, 1, CardStatus::Sarcastic);
}

#[test]
fn understands_a_baton_discard_on_us() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "g4", "y2", "b2"],
            &["r1", "y4", "p4", "y3"],
            &["y1", "r4", "b4", "b1"],
        ],
        Setup {
            discarded: &["r4"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues blue to Donald"); // getting b1
    take_turn(&mut game, "Bob clues 4 to Donald"); // saving r4
    take_turn(&mut game, "Cathy clues yellow to Donald"); // getting y1
    take_turn_draw(&mut game, "Donald discards b4", "b3");
    // Donald performed a Baton Discard on Alice.
    has_infs(&game, None, Alice, 1, &["b4"]);
    has_status(&game, Alice, 1, CardStatus::Sarcastic);
}

#[test]
fn doesn_t_baton_to_a_locked_hand() {
    let game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b3", "g5", "y2", "b2", "r2"],
        ],
        Setup {
            clue_tokens: 4,
            init: Box::new(|g| {
                pre_clue(g, Bob, 2, &["5"]);
                pre_clue(g, Bob, 3, &["2"]);
                pre_clue(g, Bob, 4, &["2"]);
                pre_clue(g, Bob, 5, &["2"]);
                fully_known(g, Alice, 3, "b3");
            }),
            ..Setup::default()
        },
    );
    // Alice should discard chop instead of performing a Baton Discard.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][4]
        }
    );
}

#[test]
fn understands_a_certain_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y3", "g4", "y2", "r4", "b3"],
            &["r3", "b1", "r2", "b5", "y3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays r2 (slot 5)");
    take_turn(&mut game, "Bob clues blue to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 4)"); // 5 Save on slot 4, [r, !5] in slot 5
    take_turn(&mut game, "Alice clues red to Bob"); // Composition f, getting r3 on Cathy's finesse
    take_turn_draw(&mut game, "Bob discards b3", "y1");
    take_turn_draw(&mut game, "Cathy discards r3", "g2");
    has_infs(&game, None, Alice, 5, &["r3"]);
}

#[test]
fn performs_a_certain_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p3", "p3", "p5", "y4", "r3"],
            &["y3", "g4", "y2", "r4", "b3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![2, 0, 0, 0, 0]),
            clue_tokens: 5,
            init: Box::new(|g| {
                pre_clue(g, Bob, 5, &["red"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues red to Cathy"); // Composition f, getting r3 on Alice's finesse
    take_turn_draw(&mut game, "Cathy discards b3", "y1");
    // Alice should certain discard slot 1 as r3.
    assert_eq!(
        game.take_action(),
        PerformAction::Discard {
            target: game.state.hands[Alice.ordinal()][0]
        }
    );
}

#[test]
fn recognizes_an_illegal_layered_certain_finesse() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["y3", "g4", "y2", "r4", "b3"],
            &["b1", "r3", "r2", "g3", "b5"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays r2 (slot 5)");
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 4)");
    take_turn(&mut game, "Alice clues red to Bob");
    // Cathy can't perform a Certain Discard to Alice's unknown red, so this is illegal.
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn correctly_resolves_a_complex_certain_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "b4", "y4", "g4"],
            &["g2", "r4", "y3", "b5"],
            &["g1", "g3", "r2", "b1"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![4, 0, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 1 to Alice (slots 2,4)");
    take_turn(&mut game, "Alice clues green to Bob"); // Quadruple finesse
    take_turn(&mut game, "Bob clues 5 to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 3)"); // Alice: xx x1 x5 x1
    take_turn_draw(&mut game, "Donald discards g1", "r3"); // Donald passes g1 to Alice
    take_turn(&mut game, "Alice plays p1 (slot 2)");
    let g4 = game.state.expand_short("g4").unwrap();
    assert!(game.waiting().iter().any(|w| w.inference == g4));
    take_turn(&mut game, "Bob clues blue to Donald");
    take_turn_draw(&mut game, "Cathy discards y3", "p5");
    take_turn(&mut game, "Donald clues 5 to Alice (slot 3)"); // Play clue on g5
    take_turn(&mut game, "Alice plays g1 (slot 4)");
    let g4 = game.state.expand_short("g4").unwrap();
    assert!(game.waiting().iter().any(|w| w.inference == g4));
    take_turn_draw(&mut game, "Bob discards y4", "y2");
    take_turn_draw(&mut game, "Cathy plays g2", "p3");
    take_turn_draw(&mut game, "Donald plays g3", "p1");
    has_infs(&game, None, Alice, 4, &["r5", "g5"]);
}

#[test]
fn correctly_resolves_a_fake_certain_discard() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "r4", "y3", "g5"],
            &["g1", "g3", "r2", "b1"],
            &["b2", "y5", "b5", "b4"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues blue to Alice (slots 1,2,3)");
    take_turn(&mut game, "Alice plays b1 (slot 1)");
    take_turn(&mut game, "Bob clues 4 to Donald"); // Looks like b2 self-finesse
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn_draw(&mut game, "Donald discards b2", "g2"); // Donald proves Alice is prompted for b2,b3
    has_infs(&game, None, Alice, 2, &["b2"]);
    has_infs(&game, None, Alice, 3, &["b3"]);
}

#[test]
fn doesn_t_discard_a_possibly_on_certain_finesse() {
    let mut game = setup(
        hgroup(10),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["p1", "y4", "g4", "b4", "p4"],
            &["r4", "y4", "g4", "b4", "p2"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["r3"],
            init: Box::new(|g| {
                pre_clue(g, Cathy, 5, &["2"]);
            }),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 5)"); // either r3 save, or p123 reverse+self certain finesse
    assert!(
        !game
            .xmeta(game.state.hands[Alice.ordinal()][0])
            .id_uncertain
    );
    assert!(
        game.xmeta(game.state.hands[Alice.ordinal()][0])
            .f_status
            .contains(&FStatus::PossiblyOn(0))
    );
    take_turn(&mut game, "Alice discards r4 (slot 4)");
    let mut no_f_game = game.clone();
    take_turn(&mut no_f_game, "Bob clues green to Alice (slot 1)");
    has_status(&no_f_game, Alice, 2, CardStatus::None);
    has_infs(&no_f_game, None, Alice, 5, &["r3"]);
    let mut f_game = game.clone();
    take_turn_draw(&mut f_game, "Bob plays p1", "g1");
    has_status(&f_game, Alice, 2, CardStatus::Finessed);
    assert!(
        f_game
            .xmeta(f_game.state.hands[Alice.ordinal()][1])
            .f_status
            .is_empty()
    );
    has_infs(&f_game, None, Alice, 5, &["p3"]);
}
