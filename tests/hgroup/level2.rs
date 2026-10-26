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
fn understands_a_continuing_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["y5", "r5", "b1", "p3"],
            &["p4", "b3", "r2", "p1"],
            &["r3", "r1", "y4", "p1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Donald");
    take_turn(&mut game, "Bob clues red to Cathy"); // r2 connection
    take_turn(&mut game, "Cathy clues 4 to Alice (slots 2,3)"); // r4, finessing Donald
    take_turn(&mut game, "Donald clues red to Bob"); // r5
    assert!(
        !game.common.thoughts[game.state.hands[Alice.ordinal()][0]]
            .inferred
            .is_empty()
    );
    assert_eq!(game.common.hypo_stacks[Colour::Red.ordinal()], 5);
}

#[test]
fn self_prompts_if_impossible_to_be_direct() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "g1", "y2", "g4", "b4"],
            &["r1", "g3", "b3", "r5", "m3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 0, 1, 0, 0]),
            variant: TestVariant::Rainbow5,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 2 to Alice (slots 1,2)");
    take_turn(&mut game, "Alice plays g2 (slot 1)");
    take_turn(&mut game, "Bob clues red to Alice (slots 2,5)"); // slot 5 is !2, thus [r1,m1,r3]
    let g = &game;
    has_infs(g, None, Alice, 5, &["r1", "m1", "r3"]);
    take_turn_draw(&mut game, "Cathy plays r1", "b1");
    has_infs(&game, None, Alice, 5, &["r3"]);
}

#[test]
fn recognizes_known_finesses() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "r4", "r4", "g4", "g4"],
            &["y4", "y4", "b4", "b4", "r3"],
        ],
        Setup {
            starting: Cathy,
            discarded: &["r1", "r1"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slot 1)");
    // Bob's r1 is definitely finessed.
    assert!(game.is_definite(game.state.hands[Bob.ordinal()][0]));
    has_status(&game, Bob, 1, CardStatus::Finessed);
    // Alice knows that her card is exactly r2.
    has_infs(&game, Some(Alice), Alice, 1, &["r2"]);
}

#[test]
fn doesn_t_self_prompt_when_a_clue_could_be_direct() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g5", "b4", "r4", "r5", "g2"],
            &["b2", "y3", "r4", "p2", "p3"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues blue to Alice (slots 4,5)");
    take_turn(&mut game, "Alice plays b1 (slot 5)");
    take_turn(&mut game, "Bob clues blue to Alice (slots 1,5)");
    // Alice's slot 1 should only be [b2,b3], not [b2,b3,b4].
    has_infs(&game, Some(Alice), Alice, 1, &["b2", "b3"]);
}

#[test]
fn waits_for_a_reverse_finesse_even_when_it_could_be_direct() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["g1", "b2", "y4", "r3", "r5"],
            &["g5", "y3", "r4", "p2", "r3"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![0, 2, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slot 2)");
    // While Alice's slot 2 could be y3, it could also be g3 (reverse finesse on Bob + self-finesse).
    has_infs(&game, None, Alice, 2, &["y3", "g3"]);
}

#[test]
fn assumes_direct_play_over_a_stomped_finesse_involving_a_self_component() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r5", "b2", "y4", "r3"],
            &["g5", "y3", "r4", "p2"],
            &["g1", "r3", "y2", "b3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 2, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Alice (slot 4)");
    take_turn(&mut game, "Cathy clues green to Donald");
    // Alice should assume the simpler explanation that she doesn't have to play g2.
    has_infs(&game, None, Alice, 4, &["y3"]);
    has_status(&game, Alice, 1, CardStatus::None);
}

#[test]
fn allows_connecting_through_multiple_possibilities() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "b4", "g3", "r4"],
            &["g4", "y3", "r4", "p2"],
            &["g1", "g5", "y1", "b4"],
        ],
        Setup {
            starting: Bob,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 1 to Donald");
    take_turn(&mut game, "Cathy clues 3 to Bob");
    // There should be y2 -> y3 and g2 -> g3 waiting connections.
    assert!(
        game.waiting()
            .iter()
            .any(|w| w.inference == Identity::new(Colour::Yellow.ordinal(), 3))
    );
    assert!(
        game.waiting()
            .iter()
            .any(|w| w.inference == Identity::new(Colour::Green.ordinal(), 3))
    );
}

#[test]
fn prefers_not_starting_with_self_even_with_known_playables_before() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p5", "b2", "y4", "r3"],
            &["g1", "y3", "r4", "p2"],
            &["g2", "p4", "y5", "b4"],
        ],
        Setup {
            play_stacks: Some(vec![0, 2, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Alice clues green to Donald");
    take_turn(&mut game, "Bob clues 4 to Alice (slot 2)");
    take_turn_draw(&mut game, "Cathy plays g1", "r1");
    take_turn_draw(&mut game, "Donald plays g2", "b1");
    has_infs(&game, None, Alice, 2, &["y4"]);
    // Alice's slot 1 is not playable (yet).
    assert!(
        game.common
            .thinks_playables(&game, Alice.ordinal(), false, true)
            .is_empty()
    );
}

#[test]
fn prefers_not_starting_with_self_symmetrically_even_with_known_playables_before() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "y2", "r4", "p2"],
            &["g5", "p4", "y5", "y3"],
            &["b2", "b2", "y4", "r3"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 2, 1, 0, 0]),
            discarded: &["r3", "y3"],
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 3 to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Alice (slot 4)");
    take_turn(&mut game, "Donald clues green to Bob");
    take_turn(&mut game, "Alice clues 4 to Donald");
    // g4 (g2 known -> g3 finesse, self) requires a self-component, compared to y3 (prompt) which does not.
    has_infs(&game, None, Donald, 3, &["y4"]);
}

#[test]
fn includes_the_correct_interpretation_even_if_it_requires_more_blind_plays() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g2", "g3", "g4", "r3"],
            &["g4", "y3", "r4", "p2"],
            &["g1", "b4", "y5", "y2"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 1, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 2 to Donald");
    take_turn(&mut game, "Cathy clues 4 to Bob");
    // There should be y2 -> y3 and g2 -> g3 -> g4 waiting connections.
    assert!(
        game.waiting()
            .iter()
            .any(|w| w.inference == Identity::new(Colour::Yellow.ordinal(), 4))
    );
    assert!(
        game.waiting()
            .iter()
            .any(|w| w.inference == Identity::new(Colour::Green.ordinal(), 4))
    );
}

#[test]
fn connects_when_a_card_plays_early() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["p5", "y4", "r1", "b3"],
            &["p1", "g5", "y3", "y1"],
            &["g1", "p2", "y1", "b5"],
        ],
        Setup {
            starting: Donald,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Alice (slot 4)");
    take_turn(&mut game, "Alice clues 1 to Donald");
    take_turn(&mut game, "Bob clues 3 to Cathy");
    take_turn(&mut game, "Cathy clues 5 to Donald");
    take_turn_draw(&mut game, "Donald plays y1", "r4");
    // Note at level 5, Alice can't play y2 since it could be a hidden finesse.
    take_turn(&mut game, "Alice plays y2 (slot 4)");
    has_infs(&game, None, Cathy, 3, &["y3"]);
}

#[test]
fn connects_to_a_finesse_after_a_fake_finesse_was_just_disproven() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["b2", "y4", "r1", "b3", "y1"],
            &["g1", "y3", "p4", "y1", "b5"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![2, 1, 1, 1, 2]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 3 to Alice (slots 2,5)");
    has_infs(&game, None, Alice, 5, &["r3", "b3", "p3"]);
    take_turn(&mut game, "Alice clues 5 to Cathy");
    take_turn(&mut game, "Bob clues purple to Cathy");
    let _g = &game;
    // Alice's slot 2 can be any 3 (not prompted to be p3).
    has_infs(&game, None, Alice, 5, &["p3"]);
    has_infs(&game, None, Alice, 2, &["r3", "y3", "g3", "b3", "p3"]);
    take_turn_draw(&mut game, "Cathy discards y1", "p1");
    take_turn(&mut game, "Alice plays r3 (slot 5)");
    // p3 must be in slot 3 (previously slot 2).
    has_infs(&game, None, Alice, 3, &["p3"]);
}

#[test]
fn doesn_t_consider_already_finessed_possibilities() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r2", "y4", "r1", "b5"],
            &["r3", "y3", "p4", "y1"],
            &["y1", "b3", "b2", "r4"],
            &["p4", "p1", "r1", "y2"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 1, 1, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues red to Emily"); // known r1
    take_turn(&mut game, "Emily clues red to Donald"); // r4 double finesse
    take_turn(&mut game, "Alice clues 5 to Bob");
    take_turn(&mut game, "Bob clues 2 to Alice (slot 2)");
    // Alice's slot 2 shouldn't be r2.
    has_infs(&game, None, Alice, 2, &["y2", "g2"]);
}

#[test]
fn doesn_t_give_continuation_clues_when_cards_are_in_a_superposition() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["r1", "r1", "p5", "r2"],
            &["r3", "g3", "p2", "p1"],
            &["b4", "y2", "r4", "p1"],
        ],
        Setup {
            starting: Cathy,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues red to Alice (slots 3,4)");
    take_turn(&mut game, "Donald clues purple to Cathy");
    take_turn(&mut game, "Alice clues red to Donald");
    assert_eq!(game.last_move(), Some(Interp::Clue(ClueInterp::Mistake)));
}

#[test]
fn correctly_identifies_the_simplest_connection_when_a_prompt_makes_the_difference() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b3", "y3", "p4", "g1"],
            &["g1", "r3", "p3", "r5"],
            &["b1", "g3", "y2", "p4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 0, 0, 1, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 2 to Alice (slots 1,3)"); // g2, b2
    take_turn(&mut game, "Alice clues 5 to Cathy");
    take_turn(&mut game, "Bob clues 3 to Donald"); // confirming we have g2
    take_turn_draw(&mut game, "Cathy plays g1", "b1");
    take_turn(&mut game, "Donald clues 4 to Alice (slot 2)");
    take_turn(&mut game, "Alice plays g2 (slot 1)"); // FIXME: Alice seems to know too much before this play; this play can even be b2!
    // assertEquals(game.common.thinksPlayables(game, Alice.ordinal), Vector(3))
    has_infs(&game, None, Alice, 2, &["g4"]);
}

#[test]
fn updates_hypo_stacks_correctly() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "p4", "g3", "g3"],
            &["p2", "y3", "y3", "b3"],
            &["g4", "g4", "r4", "r4"],
        ],
        Setup {
            starting: Donald,
            play_stacks: Some(vec![0, 0, 0, 1, 1]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Donald clues 4 to Bob");
    assert_eq!(game.common.hypo_stacks, vec![0, 0, 0, 1, 4]);
}

#[test]
fn restores_inferences_after_a_fake_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["b1", "p4", "g3", "g3"],
            &["p4", "y3", "y3", "b3"],
            &["g4", "g4", "r4", "r5"],
        ],
        Setup {
            starting: Cathy,
            play_stacks: Some(vec![1, 1, 0, 0, 0]),
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Cathy clues 1 to Alice (slots 3,4)");
    take_turn(&mut game, "Donald clues 2 to Alice (slot 1)"); // could be finessing Bob's b1
    let g = &game;
    has_infs(g, None, Alice, 1, &["r2", "y2", "g2", "b2", "p2"]);
    take_turn(&mut game, "Alice plays g1 (slot 4)"); // remaining 1 is [p1] if Bob finesses
    take_turn(&mut game, "Bob clues 5 to Donald"); // Bob doesn't play
    has_infs(&game, None, Alice, 4, &["b1", "p1"]);
}

#[test]
fn interprets_a_prompt_after_a_fake_finesse() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx"],
            &["g1", "g4", "y3", "y3"],
            &["r1", "y4", "b4", "r3"],
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
fn allows_playing_non_urgently_when_fully_known() {
    let mut game = setup(
        hgroup(2),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r2", "g4", "g4", "b4", "b4"],
            &["r1", "y4", "p4", "r4", "y1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues red to Bob");
    take_turn(&mut game, "Bob clues 1 to Cathy");
    take_turn_draw(&mut game, "Cathy plays y1", "y4");
    // The r1 -> r2 connection is still on.
    assert!(!game.waiting().is_empty());
}

#[test]
fn plays_into_a_reverse_finesse() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r3", "g3", "y4", "b3", "b4"],
            &["g1", "y3", "y3", "r1", "p4"],
        ],
        Setup {
            starting: Bob,
            play_stacks: Some(vec![0, 0, 0, 0, 2]),
            discarded: &["y4"],
            clue_tokens: 4,
            ..Setup::default()
        },
    );
    take_turn(&mut game, "Bob clues 4 to Cathy");
    take_turn_draw(&mut game, "Cathy discards r1", "r5");
    // hasInfs(game, None, Cathy, 5, Vector("r4", "y4"))
    has_infs(&game, None, Alice, 1, &["p3"]);
    has_status(&game, Alice, 1, CardStatus::Finessed);
}

#[test]
fn understands_when_others_give_selfish_clues() {
    let mut game = setup(
        hgroup(4),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r4", "y4", "g4", "b3", "p4"],
            &["r3", "y3", "b1", "y1", "r1"],
        ],
        Setup { ..Setup::default() },
    );
    take_turn(&mut game, "Alice clues 1 to Cathy");
    take_turn(&mut game, "Bob clues blue to Alice (slot 3)");
    take_turn(&mut game, "Cathy clues green to Alice (slot 1)");
    take_turn(&mut game, "Alice plays g1 (slot 1)"); // Allow Alice's slot 1 to be blue
    take_turn(&mut game, "Bob clues 5 to Alice (slot 5)");
    take_turn(&mut game, "Cathy clues blue to Bob");
    // Alice is not finessed for b1.
    has_status(&game, Alice, 1, CardStatus::None);
}
