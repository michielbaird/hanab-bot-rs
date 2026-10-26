use super::action::{Action, ClueAction, PlayAction, TurnAction};
use super::card::{CardStatus, Identifiable, Identity};
use super::clue::ClueKind;
use super::game::Game;
use super::identity_set::IdentitySet;
use super::player::Player;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FixResult {
    /// A "conventional" fix clue, indicating either trash or a duplicate (or both).
    Normal {
        clued_resets: Vec<usize>,
        duplicate_reveals: Vec<usize>,
    },
    /// A fix clue that gives no new information. The convention must decide which order is fixed (based on play order).
    NoNewInfo,
    /// Not a fix clue.
    None,
}

/// Whether a fix clue was just given.
///
/// * `prev`: the game state before the clue.
/// * `game`: the game state after the clue (and associated elim).
pub fn check_fix(prev: &Game, game: &Game, action: &ClueAction) -> FixResult {
    let list = &action.list;
    let (mut clued_resets, mut duplicate_reveals) = (Vec::new(), Vec::new());

    for &order in list {
        let duplicated = || {
            prev.state.deck[order].clued
                && game.common.thoughts[order].id().is_some()
                && list.iter().any(|&o| {
                    o != order
                        && prev.state.deck[o].clued
                        && !prev.common.thoughts[o].reset
                        && game.state.deck[order].matches(&game.state.deck[o])
                        && prev.common.thoughts[order].id().is_none()
                })
        };

        let status = prev.meta[order].status;

        if prev.common.order_trash(game, order) {
            continue;
        }

        let blind_playing_trash =
            matches!(status, CardStatus::CalledToPlay | CardStatus::DragToPlay)
                && prev.is_blind_playing(order)
                && game.common.thoughts[order]
                    .info_lock
                    .exists(|ids| ids.iter().all(|id| game.state.is_basic_trash(id)));
        let clued_kt = prev.state.deck[order].clued
            && !prev.common.thoughts[order].reset
            && game.common.order_kt(game, order);

        if blind_playing_trash || clued_kt {
            clued_resets.push(order);
        } else if duplicated() {
            duplicate_reveals.push(order);
        }
    }

    if clued_resets.is_empty() && duplicate_reveals.is_empty() {
        FixResult::None
    } else {
        FixResult::Normal {
            clued_resets,
            duplicate_reveals,
        }
    }
}

/// If `id` is provided, returns a non-empty list if it can be made playable by `target`'s turn.
/// Otherwise, returns the orders of playable cards in `target`'s hand by their turn.
///
/// * `player`: the observing player (determines which cards are playable).
/// * `start`: the index of the first player allowed to play a card.
/// * `target`: the index of the destination player (they do not play a card).
pub fn connectable_simple(
    game: &Game,
    player: &Player,
    start: usize,
    target: usize,
    id: Option<Identity>,
) -> Vec<usize> {
    let state = &game.state;

    if id.is_some_and(|id| state.is_playable(id)) {
        return vec![99];
    }
    if start == target {
        return player.obvious_playables(game, target);
    }
    if state.ended() {
        return Vec::new();
    }

    let next_player_index = state.next_player_index(start);

    for order in player.obvious_playables(game, start) {
        let Some(play_id) = player.thoughts[order].id_with(true, false) else {
            continue;
        };

        let mut new_game = game.clone();
        new_game.no_recurse = true;
        if state.current_player_index != start {
            // Go to starting player's turn
            let turn = TurnAction {
                num: state.turn_count,
                current_player_index: Some(start),
            };
            new_game = new_game.simulate_action(&Action::Turn(turn), None, false);
        }
        let play = PlayAction {
            player_index: start,
            order,
            id: Some(play_id),
        };
        new_game = new_game.simulate_action(&Action::Play(play), None, false);

        let connected = connectable_simple(
            &new_game,
            new_game.view(player),
            next_player_index,
            target,
            id,
        );
        if !connected.is_empty() {
            return connected;
        }
    }

    connectable_simple(game, player, next_player_index, target, id)
}

/// The possible ids of a distribution clue.
pub fn distribution_clue(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    focus: usize,
) -> Option<IdentitySet> {
    let state = &game.state;
    let ClueAction {
        target, list, clue, ..
    } = action;
    let thought = &game.common.thoughts[focus];

    if list.iter().all(|&o| prev.state.deck[o].clued)
        || (!game.in_endgame() && state.rem_score() > state.variant.suits.len())
    {
        return None;
    }

    if state.deck[focus]
        .id()
        .is_some_and(|id| state.is_basic_trash(id))
    {
        return None;
    }

    let poss = if clue.kind == ClueKind::Colour {
        thought.possible
    } else {
        thought.possible.filter(|id| id.rank == clue.value)
    };

    let mut useful = IdentitySet::EMPTY;
    for id in poss {
        let duplicated = || {
            state.hands.iter().enumerate().any(|(i, hand)| {
                i != *target
                    && hand
                        .iter()
                        .any(|&o| game.is_touched(o) && game.order_matches(o, id, true))
            })
        };

        if state.is_basic_trash(id) || state.is_critical(id) {
            continue;
        }
        if duplicated() {
            useful.insert(id);
        } else {
            return None;
        }
    }

    (!useful.is_empty()).then_some(useful)
}

/// Whether the prompted card should be ignored as a *Free Choice Finesse*.
///
/// * `game`: the game state (after the clue).
/// * `id`: the identity that this card would be prompted as.
/// * `prompt`: the order of the prompted card.
pub fn rainbow_mismatch(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    id: Identity,
    prompt: usize,
) -> bool {
    use super::variant::RAINBOWISH;

    let state = &game.state;
    let ClueAction {
        target, list, clue, ..
    } = action;

    let skip = clue.kind != ClueKind::Colour
        || !state.variant.suits[id.suit_index].suit_type.rainbowish
        || prev.known_as(prompt, RAINBOWISH, None)
        || state.deck[prompt]
            .clues
            .iter()
            .any(|c| super::clue::ClueLike::is_eq(c, clue));

    if skip {
        return false;
    }

    let all_rainbow = list.iter().all(|&o| {
        if *target == state.our_player_index {
            game.me().thoughts[o]
                .possible
                .iter()
                .all(|c| state.variant.suits[c.suit_index].suit_type.rainbowish)
        } else {
            let known = state.deck[o]
                .identity
                .expect("other players' cards are visible");
            state.variant.suits[known.suit_index].suit_type.rainbowish
        }
    });

    if !all_rainbow {
        return false;
    }

    // A matching colour would have touched the same cards.
    let mut sorted_list = list.clone();
    sorted_list.sort_unstable();

    *target == state.our_player_index
        || state.deck[prompt].clues.iter().any(|c| {
            let mut touched = state.clue_touched(&state.hands[*target], c);
            touched.sort_unstable();
            touched == sorted_list
        })
}
