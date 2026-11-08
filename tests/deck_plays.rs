//! Deck Plays: the last card of the deck can be played (or bombed) without ever having been drawn.

use std::collections::HashMap;

use hanab_bot::basics::{Game, Identifiable, Identity, PerformAction};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game, load_variant};

/// A real 3-player game (`HGroup2`, replay 2045738) cut after 55 of its 59 actions, when exactly one card is left in
/// the deck, with Deck Plays on and the first player playing that last card. `identity` says which card it is.
fn deck_play(
    convention: ConventionKind,
    identity: impl Fn(&Game) -> Identity,
) -> (Game, Game, Identity) {
    logger::set_level(LogLevel::Off);
    let path = format!(
        "{}/investigations/replay_2045738.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut data = GameData::from_file(&path).expect("the saved replay");
    data.actions.truncate(55);
    data.options.deck_plays = true;
    let variant = load_variant(&data.options.variant_name, &HashMap::new()).unwrap();

    let before = build_game(convention, &data, variant.clone(), 0, false).unwrap();
    assert_eq!(before.state.cards_left, 1, "one card is left in the deck");
    let last = data.deck.len() - 1;
    assert_eq!(before.state.deck.len(), last, "the last card is not drawn");

    let id = identity(&before);
    data.deck[last] = id;
    data.actions.push(PerformAction::Play { target: last });
    let after = build_game(convention, &data, variant, 0, false).expect("the deck play replays");
    (before, after, id)
}

#[test]
fn a_successful_deck_play_scores() {
    let (before, after, id) = deck_play(ConventionKind::HGroup(2), |g| {
        g.state.playable_set.first().expect("something is playable")
    });

    assert_eq!(after.state.score(), before.state.score() + 1);
    assert_eq!(after.state.strikes, before.state.strikes);
    assert_eq!(after.state.deck.len(), 50, "the card entered the game");
    assert_eq!(after.state.deck[49].id(), Some(id));
    assert_eq!(after.state.cards_left, 0);
}

#[test]
fn a_failed_deck_play_is_a_strike() {
    let (before, after, id) = deck_play(ConventionKind::HGroup(2), |g| {
        // Something that is not playable (and, for what it is worth, already played)
        g.state.trash_set.first().expect("something is trash")
    });

    assert!(!before.state.is_playable(id));
    assert_eq!(after.state.score(), before.state.score());
    assert_eq!(after.state.strikes, before.state.strikes + 1);
    assert_eq!(after.state.deck.len(), 50, "the card entered the game");
    assert_eq!(after.state.cards_left, 0);
}

#[test]
fn a_deck_play_can_be_analysed() {
    // (The analysis plays the game through again, and looks at what the bot would have done instead, with the endgame
    // solver: it must not trip over a card that was never drawn.)
    for (success, convention) in [
        (false, ConventionKind::HGroup(2)),
        (true, ConventionKind::Reactor),
    ] {
        let (_, after, _) = deck_play(convention, |g| {
            if success {
                g.state.playable_set.first().unwrap()
            } else {
                g.state.trash_set.first().unwrap()
            }
        });
        let _ = after.analyze_comments();
    }
}
