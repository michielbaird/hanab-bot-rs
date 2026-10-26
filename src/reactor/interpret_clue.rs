//! Interpretation of clues: stable clues (the receiver reacts) and the fallback to reactive clues.

use super::interpret_reactive::{interpret_reactive_colour, interpret_reactive_rank};
use super::{ReactorGame, ReactorWC};
use crate::basics::action::{Action, ClueAction, add_action};
use crate::basics::card::CardStatus;
use crate::basics::clue::{Clue, ClueKind};
use crate::basics::clue_result::bad_touch_result;
use crate::basics::fix::{FixResult, check_fix, connectable_simple};
use crate::basics::interp::{ClueInterp, Interp};
use crate::basics::variant::MUDDY;
use crate::basics::{Game, Identifiable, Identity, IdentitySet, State};
use crate::logger;
use crate::util::{first_max_by_key, players_until};
use crate::{log_highlight, log_info, log_warn};

#[allow(clippy::large_enum_variant)]
pub enum StableResult {
    /// A valid stable clue. Includes the interpretation and the updated game.
    Stable(ClueInterp, Game),
    /// The reacter knows that this stable clue doesn't work, and will try to react.
    TryReactive,
    /// The reacter doesn't know that this stable clue doesn't work.
    Mistake,
}

/// The 1-indexed slot of the receiver's hand that the reacter's response is relative to.
pub fn reactive_focus(state: &State, receiver: usize, action: &ClueAction) -> usize {
    let ClueAction { list, clue, .. } = action;
    let hand = &state.hands[receiver];

    let focus_index = || {
        let (index, _) = first_max_by_key(
            hand.iter().enumerate().filter(|&(_, o)| list.contains(o)),
            |&(_, &o)| if o == hand[0] { -1 } else { o as isize },
        )
        .expect("clue touched nothing");
        index
    };

    match clue.kind {
        ClueKind::Colour => {
            if state.variant.rainbowish || state.variant.rainbow_s {
                clue.value + 1
            } else {
                focus_index() + 1
            }
        }
        ClueKind::Rank => {
            if state.variant.pinkish || state.variant.pink_s {
                clue.value
            } else {
                focus_index() + 1
            }
        }
    }
}

pub fn interpret_stable(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    stall: bool,
) -> (ClueInterp, Game) {
    let ClueAction { giver, target, .. } = *action;
    let bob = game.state.next_player_index(giver);

    let reactive_game = || {
        let mut hypo = prev.clone();
        let turn = hypo.state.turn_count;
        add_action(
            std::sync::Arc::make_mut(&mut hypo.state.action_list),
            Action::Clue(action.clone()),
            turn,
        );
        hypo.on_clue(action);
        hypo.elim(None, false);

        interpret_reactive(prev, &hypo, action, bob, true)
    };

    match try_stable(prev, game, action, stall) {
        StableResult::TryReactive if target != bob => reactive_game(),

        StableResult::Stable(interp, new_game) => {
            if target != bob && bad_stable(prev, &new_game, action, interp, stall) {
                reactive_game()
            } else {
                (interp, new_game)
            }
        }

        _ => (ClueInterp::Mistake, game.clone()),
    }
}

fn try_stable(prev: &Game, game: &Game, action: &ClueAction, stall: bool) -> StableResult {
    log_info!("interpreting stable clue!");
    let state = &game.state;
    let ClueAction {
        giver,
        target,
        ref list,
        clue,
    } = *action;
    let new_touched: Vec<usize> = list
        .iter()
        .copied()
        .filter(|&o| !prev.state.deck[o].clued)
        .collect();
    let next_player_index = state.next_player_index(giver);
    let num_suits = state.variant.suits.len();

    let mut g = game.clone();

    if clue.kind == ClueKind::Rank && !new_touched.is_empty() {
        let trash_push = (0..num_suits)
            .all(|suit_index| state.is_basic_trash(Identity::new(suit_index, clue.value)));

        let playable_rank_focus = || {
            if state.variant.pinkish {
                g.state.hands[target]
                    .iter()
                    .copied()
                    .filter(|&o| !prev.state.deck[o].clued)
                    .min()
                    .filter(|o| list.contains(o))
                    .unwrap_or_else(|| *new_touched.iter().max().expect("not empty"))
            } else {
                *new_touched.iter().max().expect("not empty")
            }
        };

        let playable_rank = |focus: usize| {
            (0..num_suits).all(|suit_index| {
                let id = Identity::new(suit_index, clue.value);
                !game.common.thoughts[focus].possible.contains(id)
                    || state.is_basic_trash(id)
                    || state.is_playable(id)
            })
        };

        if trash_push {
            let focus = *new_touched.iter().max().expect("not empty");
            g.common.update_thought(focus, |t| {
                t.inferred = t.inferred.intersect(state.trash_set)
            });
            g.meta[focus].trash = true;
        } else {
            let focus = playable_rank_focus();

            if playable_rank(focus) {
                let unnecessary_focus = g.common.thoughts[focus].possible.iter().all(|i| {
                    state.is_basic_trash(i)
                        || state
                            .held_orders
                            .iter()
                            .any(|o| g.common.thoughts[o].matches(&i))
                });

                if unnecessary_focus {
                    log_info!("unnecessary focus!");
                } else {
                    let thought = &g.common.thoughts[focus];
                    let new_inferred = thought
                        .inferred
                        .filter(|i| state.is_playable(i) && i.rank == clue.value)
                        .when_empty(|| {
                            thought
                                .possible
                                .filter(|i| state.is_playable(i) && i.rank == clue.value)
                        });

                    log_info!(
                        "playable rank clue on {focus}! ({})",
                        new_inferred.fmt(state)
                    );

                    g.common.update_thought(focus, |t| {
                        t.inferred = new_inferred;
                        t.info_lock = new_inferred.into();
                    });
                    g.meta[focus].focused = true;
                    g.meta[focus].status = CardStatus::CalledToPlay;
                    g.meta[focus].by = Some(giver);
                }
            }
        }
    }

    // (This overwrites any existing waiting connection with `None`, as the Scala bot does.)
    g.reactor_mut().waiting =
        (game.waiting().is_none() && next_player_index != target).then(|| {
            log_info!("writing potential response inversion!");

            ReactorWC {
                giver,
                reacter: next_player_index,
                receiver: target,
                receiver_hand: state.hands[target].clone(),
                clue,
                focus_slot: reactive_focus(state, target, action),
                inverted: true,
                turn: state.turn_count,
            }
        });

    if let FixResult::Normal {
        clued_resets,
        duplicate_reveals,
    } = check_fix(prev, &g, action)
    {
        log_info!("fix clue! {clued_resets:?} {duplicate_reveals:?}");
        return StableResult::Stable(ClueInterp::Fix, g);
    }

    let dedup = |mut orders: Vec<usize>| {
        let mut seen = Vec::new();
        orders.retain(|o| {
            let new = !seen.contains(o);
            seen.push(*o);
            new
        });
        orders
    };

    let prev_playables = dedup(
        prev.common
            .obvious_playables(prev, target)
            .into_iter()
            .chain(connectable_simple(
                prev,
                &prev.players[giver],
                next_player_index,
                target,
                None,
            ))
            .collect(),
    );

    let playables = {
        let mut play_game = g.clone();
        play_game.with_move(ClueInterp::Play.into(), false);

        dedup(
            g.common
                .obvious_playables(&g, target)
                .into_iter()
                .chain(connectable_simple(
                    &play_game,
                    &g.players[giver],
                    next_player_index,
                    target,
                    None,
                ))
                .collect(),
        )
    };

    log_info!("playables {playables:?}, prev_playables {prev_playables:?}");

    let revealed = || -> Vec<usize> {
        playables
            .iter()
            .copied()
            .filter(|o| {
                list.contains(o)
                    && !prev_playables.contains(o)
                    && (clue.kind == ClueKind::Rank || prev.state.deck[*o].clued)
            })
            .collect()
    };

    // Try connecting with an unknown playable
    let reveal_connectable = || -> Option<(usize, Identity)> {
        if next_player_index == target {
            return None;
        }

        let prev_plays = prev.common.obvious_playables(prev, next_player_index);
        let possible_connections: Vec<Identity> = list
            .iter()
            .copied()
            .filter(|&o| prev.state.deck[o].clued)
            .filter_map(|o| {
                let thought = &game.common.thoughts[o];
                if !game.state.has_consistent_infs(thought) {
                    return None;
                }
                let id = thought.id()?;
                (state.playable_away(id) == 1).then(|| id.prev().expect("playable away 1"))
            })
            .collect();

        prev_plays.into_iter().find_map(|o| {
            possible_connections
                .iter()
                .find(|id| g.common.thoughts[o].inferred.contains(**id))
                .map(|&id| (o, id))
        })
    };

    if new_touched.is_empty() {
        let safe_actions: Vec<usize> = playables
            .iter()
            .copied()
            .chain(
                g.common
                    .thinks_trash(&g, target)
                    .into_iter()
                    .filter(|&o| prev.state.deck[o].clued),
            )
            .collect();
        let old_safe_actions: Vec<usize> = prev_playables
            .iter()
            .copied()
            .chain(prev.common.thinks_trash(prev, target))
            .collect();

        if let Some(safe) = safe_actions.iter().find(|o| !old_safe_actions.contains(o)) {
            log_info!("revealed a safe action! {safe}");
            StableResult::Stable(ClueInterp::Reveal, g)
        } else if stall {
            log_info!("stalling with fill-in/hard burn!");
            StableResult::Stable(ClueInterp::Stall, g)
        } else {
            let connectable: Vec<usize> = {
                let mut reveal_game = game.clone();
                reveal_game.with_move(ClueInterp::Reveal.into(), false);
                connectable_simple(&reveal_game, &game.common, next_player_index, target, None)
                    .into_iter()
                    .filter(|o| !old_safe_actions.contains(o))
                    .collect()
            };

            if !connectable.is_empty() {
                log_info!("connecting to revealed playables! {connectable:?}");
                return StableResult::Stable(ClueInterp::Reveal, g);
            }

            match reveal_connectable() {
                Some((order, id)) => {
                    log_info!(
                        "connecting through unknown playable ({})!",
                        state.log_id(id)
                    );

                    if state.deck[order].matches_assume(&id) {
                        g.common
                            .update_thought(order, |t| t.inferred = IdentitySet::from(id));
                        StableResult::Stable(ClueInterp::Reveal, g)
                    } else {
                        StableResult::Mistake
                    }
                }

                None if list.iter().all(|&o| game.common.thoughts[o].id().is_some()) => {
                    // All fully known - must be reactive
                    StableResult::TryReactive
                }

                None => {
                    let focus = tempo_focus(game, action);

                    match focus {
                        Some(focus)
                            if !state.deck[focus]
                                .id()
                                .is_some_and(|id| !state.is_playable(id)) =>
                        {
                            g.common.update_thought(focus, |t| {
                                t.inferred = t.inferred.intersect(state.playable_set)
                            });
                            g.meta[focus].status = CardStatus::CalledToPlay;
                            g.meta[focus].by = Some(giver);
                            log_info!("tempo clue on {focus}!");
                            StableResult::Stable(ClueInterp::Play, g)
                        }
                        _ => {
                            log_warn!(
                                "looked like fill-in/hard burn outside of a stalling situation!"
                            );
                            StableResult::TryReactive
                        }
                    }
                }
            }
        }
    } else if !revealed().is_empty() {
        log_info!(
            "revealed a safe action! {:?} {prev_playables:?}",
            revealed()
        );
        StableResult::Stable(ClueInterp::Reveal, g)
    } else if let Some((conn_order, conn_id)) = reveal_connectable() {
        log_highlight!(
            logger::CYAN,
            "revealed a potential safe action! playing {conn_order} urgently as {} to connect",
            state.log_id(conn_id)
        );

        if state.deck[conn_order].matches_assume(&conn_id) {
            g.common.update_thought(conn_order, |t| {
                t.old_inferred = t.inferred.into();
                t.inferred = IdentitySet::from(conn_id);
            });
            g.meta[conn_order].urgent = true;
            g.meta[conn_order].status = CardStatus::CalledToPlay;
            g.meta[conn_order].by = Some(giver);
            StableResult::Stable(ClueInterp::Reveal, g)
        } else {
            StableResult::Mistake
        }
    } else if g
        .common
        .order_kt(game, *new_touched.iter().max().expect("not empty"))
    {
        // at least 1 useful unplayable brown and clue didn't touch chop
        let brownish_tcm = state.variant.brownish
            && clue.kind == ClueKind::Rank
            && !prev.common.obvious_loaded(game, target)
            && state
                .variant
                .suits
                .iter()
                .enumerate()
                .any(|(suit_index, suit)| {
                    suit.suit_type.brownish
                        && state.play_stacks[suit_index] + 1 < state.max_ranks[suit_index]
                        && !new_touched.contains(&state.hands[target][0])
                });

        if brownish_tcm {
            log_info!("brown direct discard!");
            StableResult::Stable(ClueInterp::Reveal, g)
        } else {
            log_info!("trash push!");
            ref_play(prev, &g, action)
        }
    } else if clue.kind == ClueKind::Colour {
        log_info!("colour clue!");
        ref_play(prev, &g, action)
    } else {
        log_info!("rank clue!");
        ref_discard(prev, &g, action, stall)
    }
}

/// The card a fill-in / hard burn is "really" focusing on (a tempo clue), if any.
fn tempo_focus(game: &Game, action: &ClueAction) -> Option<usize> {
    let state = &game.state;
    let ClueAction {
        target,
        ref list,
        clue,
        ..
    } = *action;
    let variant = &state.variant;

    let muddy_cards = || -> Vec<usize> {
        let special_rank = if variant.rainbow_s {
            variant.special_rank
        } else {
            None
        };
        let mut cards: Vec<usize> = list
            .iter()
            .copied()
            .filter(|&o| game.known_as(o, MUDDY, special_rank))
            .collect();
        cards.sort_by_key(|&o| std::cmp::Reverse(o));
        cards
    };

    if variant.pinkish && clue.kind == ClueKind::Rank {
        // Pink positional
        return clue
            .value
            .checked_sub(1)
            .and_then(|i| state.hands[target].get(i).copied());
    }

    if (variant.rainbow_s || variant.muddy || (variant.rainbowish && variant.pinkish))
        && clue.kind == ClueKind::Colour
    {
        let muddy = muddy_cards();
        if !muddy.is_empty() {
            // Mud clue
            let colours_available = variant.colourable_suits.len();
            let len = muddy.len();
            let focus_index = (clue.value + 6 * len - colours_available) % len;

            return muddy.get(focus_index).copied();
        }
    }

    let possible_plays = list.iter().copied().filter(|&o| {
        !game.common.thoughts[o]
            .inferred
            .intersect(state.playable_set)
            .is_empty()
    });

    if clue.kind == ClueKind::Rank
        || variant.suits[variant.colourable_suits[clue.value]]
            .suit_type
            .pinkish
    {
        possible_plays.max()
    } else {
        possible_plays.min()
    }
}

/// Returns the best alternative clue, if it exists, and whether it is a ref play.
fn alternative_clue(prev: &Game, giver: usize, target: usize) -> (Option<Clue>, bool) {
    if prev.no_recurse {
        return (None, false);
    }
    let state = &prev.state;

    let mut found_clue: Option<Clue> = None;
    let mut ref_play = false;

    for clue in state.all_valid_clues(target) {
        if found_clue.is_some() && clue.kind != ClueKind::Colour {
            continue;
        }

        let action = Action::from_clue(state, &clue, giver);
        let mut hypo_base = prev.clone();
        hypo_base.no_recurse = true;
        let hypo = hypo_base.simulate(&Action::Clue(action.clone()), Some(false));
        let (bad_touch, _, _) = bad_touch_result(prev, &hypo, &action);

        if !bad_touch.is_empty() {
            continue;
        }

        match hypo.last_move().expect("simulated clue has a move") {
            Interp::Clue(ClueInterp::Play) => return (Some(clue), true),
            Interp::Clue(ClueInterp::Reveal) => {
                found_clue = Some(clue);
                ref_play = false;
            }
            Interp::Clue(ClueInterp::Discard) => {
                let dc_target = hypo.state.hands[target]
                    .iter()
                    .copied()
                    .find(|&o| hypo.gained_status(prev, o, CardStatus::CalledToDiscard));

                // TODO(Scala): The dc target doesn't always exist. Consider a ref dc onto a card that previously had ctd.
                if dc_target.is_some_and(|dc| {
                    state.is_basic_trash(
                        state.deck[dc].id().expect("the target's cards are visible"),
                    )
                }) {
                    found_clue = Some(clue);
                }
                ref_play = false;
            }
            _ => ref_play = false,
        }
    }

    (found_clue, ref_play)
}

pub fn bad_stable(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    interp: ClueInterp,
    stall: bool,
) -> bool {
    let state = &game.state;
    let ClueAction {
        giver,
        target,
        clue,
        ..
    } = *action;

    if interp == ClueInterp::Mistake {
        return true;
    }

    let bad_playable = || {
        state.held_orders.iter().find(|&o| {
            !state.has_consistent_infs(&game.common.thoughts[o])
                && (game.gained_status(prev, o, CardStatus::CalledToPlay)
                    || (game.gained_status(prev, o, CardStatus::DragToPlay)
                        && !state.is_inverted_order(o))
                    || (game.gained_status(prev, o, CardStatus::CalledToDiscard)
                        && state.is_inverted_order(o)))
        })
    };

    let dc_target = || {
        state.hands[target].iter().copied().find(|&o| {
            game.gained_status(prev, o, CardStatus::CalledToDiscard)
                || (game.gained_status(prev, o, CardStatus::DragToPlay)
                    && game.state.is_inverted_order(o))
        })
    };

    if target == state.our_player_index {
        return false;
    }

    if let Some(bad) = bad_playable() {
        log_warn!("bad playable on {bad} {}!", state.log_order(bad));
        return true;
    }

    let dc = dc_target();
    if let Some(o) = dc
        && state.is_critical(state.deck[o].id().expect("the target's cards are visible"))
    {
        log_warn!("critical discard on {o}!");
        return true;
    }

    if game.no_recurse {
        return false;
    }

    let (alt_clue, ref_play) = alternative_clue(prev, giver, target);
    let dc_useful = dc.is_some_and(|o| {
        state.is_useful(state.deck[o].id().expect("the target's cards are visible"))
    });

    if stall && dc_useful && alt_clue.is_some() {
        log_warn!("bad discard on {dc:?}! an alternative clue was available!");
        true
    } else if prev.state.turn_count == 1
        && prev.state.clue_tokens == 8
        && clue.kind == ClueKind::Rank
        && ref_play
    {
        log_warn!("bad turn 1 rank clue! {alt_clue:?} is possible");
        true
    }
    // Check for bad lock
    else if interp == ClueInterp::Lock && alt_clue.is_some() {
        log_warn!("alternative clue {alt_clue:?} was available!");
        true
    } else if !stall {
        false
    } else if interp == ClueInterp::Stall && alt_clue.is_some() {
        log_warn!("alternative clue {alt_clue:?} was available!");
        true
    } else {
        false
    }
}

pub fn interpret_reactive(
    prev: &Game,
    game: &Game,
    action: &ClueAction,
    reacter: usize,
    looks_stable: bool,
) -> (ClueInterp, Game) {
    let state = &game.state;
    let ClueAction {
        giver,
        target: receiver,
        clue,
        ..
    } = *action;

    log_info!("interpreting reactive clue!");
    log_info!(
        "reacter: {:?} ({}), receiver: {:?} ({})",
        state.hands[reacter],
        state.names[reacter],
        state.hands[receiver],
        state.names[receiver]
    );

    let focus_slot = reactive_focus(state, receiver, action);

    let mut new_game = game.clone();
    new_game.reactor_mut().waiting = Some(ReactorWC {
        giver,
        reacter,
        receiver,
        receiver_hand: state.hands[receiver].clone(),
        clue,
        focus_slot,
        inverted: false,
        turn: state.turn_count,
    });

    if receiver == state.our_player_index {
        (ClueInterp::Reactive, new_game)
    } else {
        match clue.kind {
            ClueKind::Colour => interpret_reactive_colour(
                prev,
                &new_game,
                action,
                focus_slot,
                reacter,
                looks_stable,
            ),
            ClueKind::Rank => interpret_reactive_rank(prev, &new_game, action, focus_slot, reacter),
        }
    }
}

/// The cards that players between the giver and receiver might play (and the identity that would be played onto).
pub fn delayed_plays(
    game: &Game,
    giver: usize,
    receiver: usize,
    stable: bool,
) -> Vec<(usize, Identity)> {
    let (common, state, meta) = (&game.common, &game.state, &game.meta);
    let mut acc: Vec<(usize, Identity)> = Vec::new();

    for player_index in players_until(state.num_players, state.next_player_index(giver), receiver) {
        let playables: Vec<usize> = match state.hands[player_index]
            .iter()
            .copied()
            .find(|&o| meta[o].urgent)
        {
            // If they have an urgent discard, they can't play a connecting card.
            Some(urgent) if meta[urgent].status == CardStatus::CalledToDiscard => Vec::new(),
            // If they have an urgent playable, they can only play that card.
            Some(urgent) => vec![urgent],
            None => {
                let obvious = common.obvious_playables(game, player_index);
                // If they have more than 1 playable in a reverse reactive clue, they won't know which to play.
                // TODO(Scala): If the reacter has only one 1-away, they do know.
                if !stable && obvious.len() > 1 {
                    Vec::new()
                } else {
                    obvious
                }
            }
        };

        for &o in &playables {
            // Only consider playing the leftmost of similarly-possible cards
            if playables
                .iter()
                .any(|&p| p > o && common.thoughts[p].possible == common.thoughts[o].possible)
            {
                continue;
            }

            // The Scala list is built by prepending, so later entries come first.
            match common.thoughts[o].id_with(true, false) {
                Some(id) => acc.insert(0, (o, Identity::new(id.suit_index, id.rank + 1))),
                None => {
                    // (`IdentitySet.map` yields the identities in descending order.)
                    let group: Vec<_> = common.thoughts[o]
                        .inferred
                        .difference(state.trash_set)
                        .iter()
                        .rev()
                        .map(|i| (o, Identity::new(i.suit_index, i.rank + 1)))
                        .collect();
                    acc.splice(0..0, group);
                }
            }
        }
    }

    acc
}

pub fn ref_play(prev: &Game, game: &Game, action: &ClueAction) -> StableResult {
    let hand = &game.state.hands[action.target];
    let target = action
        .list
        .iter()
        .copied()
        .filter(|&o| !prev.state.deck[o].clued)
        .map(|o| game.common.refer(prev, hand, o, true))
        .max()
        .expect("clue newly touched nothing");

    if game.is_blind_playing(target) {
        log_warn!("targeting an already known playable!");
        StableResult::TryReactive
    } else if game.meta[target].status == CardStatus::CalledToDiscard {
        log_warn!("targeting a card called to discard!");
        StableResult::TryReactive
    } else if game.state.deck[target].clued && game.common.thinks_inverted(&game.state, target) {
        log_warn!("orange colour clue focusing chop!");

        let mut new_game = game.clone();
        new_game.common.update_thought(target, |t| {
            t.inferred = t.possible.intersect(game.state.trash_set);
            t.reset = true;
        });
        new_game.meta[target].status = CardStatus::DragToPlay;
        new_game.meta[target].trash = true;

        log_info!(
            "setting {target} to {}",
            new_game.common.str_infs(&game.state, target)
        );
        StableResult::Stable(ClueInterp::Reveal, new_game)
    } else {
        target_play(game, action, target, false, true)
    }
}

pub fn target_play(
    game: &Game,
    action: &ClueAction,
    target: usize,
    urgent: bool,
    stable: bool,
) -> StableResult {
    let state = &game.state;
    let holder = state.holder_of[target];
    let possible_conns = delayed_plays(game, action.giver, holder, stable);
    let turn = state.turn_count;

    let thought = &game.common.thoughts[target];
    let new_inferred = thought
        .inferred
        .filter(|i| state.is_playable(i) || possible_conns.iter().any(|c| c.1 == i))
        .when_empty(|| {
            thought
                .possible
                .filter(|i| state.is_playable(i) || possible_conns.iter().any(|c| c.1 == i))
        });

    let mut g = game.clone();

    if let Some(id) = state.deck[target].id()
        && let Some(&(conn_order, _)) = possible_conns.iter().find(|c| c.1 == id)
    {
        let prev_id = id.prev().expect("connecting identity has a previous rank");
        log_info!(
            "updating connecting {conn_order} as {} to be urgent",
            state.log_id(prev_id)
        );

        g.common.update_thought(conn_order, |t| {
            t.old_inferred = t.inferred.into();
            t.inferred = IdentitySet::from(prev_id);
        });
        let meta = &mut g.meta[conn_order];
        meta.urgent = true;
        meta.status = CardStatus::CalledToPlay;
        meta.by = Some(action.giver);
        meta.reason(turn);
    }

    g.common.update_thought(target, |t| {
        t.old_inferred = t.inferred.into();
        t.inferred = new_inferred;
        t.info_lock = new_inferred.into();
    });
    g.meta[target].reason(turn);
    g.meta[target].signal(turn);

    let inverted_id = state.deck[target]
        .id()
        .is_some_and(|id| state.is_inverted(id));

    if (new_inferred.is_empty() || !state.has_consistent_infs(&g.common.thoughts[target]))
        && !inverted_id
    {
        log_warn!("target {target} was reset!");

        g.common.update_thought(target, |t| t.reset_inferences());
        if action.target == state.next_player_index(action.giver) {
            if g.common.order_kt(&g, target) {
                StableResult::Stable(ClueInterp::Stall, g)
            } else {
                StableResult::Mistake
            }
        } else {
            StableResult::TryReactive
        }
    } else {
        let inverted_target = inverted_id
            || (!new_inferred.is_empty() && new_inferred.iter().all(|i| state.is_inverted(i)));

        g.meta[target].status = CardStatus::DragToPlay;
        g.meta[target].by = Some(action.giver);
        g.meta[target].focused = !inverted_target;
        g.meta[target].urgent = urgent;

        log_info!(
            "targeting play {target} ({}), infs {}{}",
            state.names[holder],
            g.common.str_infs(state, target),
            if urgent { ", urgent" } else { "" }
        );
        StableResult::Stable(
            if inverted_target {
                ClueInterp::Discard
            } else {
                ClueInterp::Play
            },
            g,
        )
    }
}

pub fn target_discard(
    game: &Game,
    action: &ClueAction,
    target: usize,
    urgent: bool,
) -> StableResult {
    let state = &game.state;
    let turn = state.turn_count;

    let mut new_game = game.clone();
    let unconventional = state
        .critical_set
        .filter(|id| !(state.is_inverted(id) && state.is_playable(id)));
    new_game.common.update_thought(target, |t| {
        t.old_inferred = t.inferred.into();
        t.inferred = t.inferred.difference(unconventional);
    });
    let meta = &mut new_game.meta[target];
    meta.status = CardStatus::CalledToDiscard;
    meta.by = Some(action.giver);
    meta.urgent = urgent;
    meta.reason(turn);
    meta.signal(turn);

    if new_game.common.thoughts[target].inferred.is_empty() {
        log_warn!("target {target} was reset!");
        StableResult::TryReactive
    } else {
        log_info!(
            "targeting discard {target} ({}), infs {}{}",
            state.names[state.holder_of[target]],
            game.common.str_infs(state, target),
            if urgent { ", urgent" } else { "" }
        );
        StableResult::Stable(ClueInterp::Discard, new_game)
    }
}

pub fn ref_discard(prev: &Game, game: &Game, action: &ClueAction, stall: bool) -> StableResult {
    let state = &game.state;
    let ClueAction {
        giver,
        target: receiver,
        ref list,
        clue,
    } = *action;
    let hand = &state.hands[receiver];
    let turn = state.turn_count;
    let new_touched: Vec<usize> = list
        .iter()
        .copied()
        .filter(|&o| !prev.state.deck[o].clued)
        .collect();

    let lock_order = hand
        .iter()
        .copied()
        .filter(|&o| !prev.state.deck[o].clued)
        .min();

    let breaks_pink_promise = |order: usize| {
        state.deck[order]
            .id()
            .is_some_and(|id| id.rank != clue.value)
    };
    let pink_failure = || {
        if state.next_player_index(giver) == receiver {
            StableResult::Mistake
        } else {
            StableResult::TryReactive
        }
    };

    match lock_order {
        Some(lock_order) if list.contains(&lock_order) => {
            if stall && state.next_player_index(receiver) == giver {
                log_info!("stall to Cathy's lock card!");
                StableResult::Stable(ClueInterp::Stall, game.clone())
            } else if prev.common.thinks_locked(prev, receiver) {
                log_info!("player was already locked!");
                StableResult::TryReactive
            } else {
                log_info!("locked!");

                let mut g = game.clone();
                if state.variant.pinkish {
                    g.common.update_thought(lock_order, |t| {
                        t.inferred = t.inferred.filter(|i| i.rank == clue.value)
                    });
                    g.meta[lock_order].focused = true;
                    // Breaks pink promise
                    if breaks_pink_promise(lock_order) {
                        return pink_failure();
                    }
                }

                for &order in hand {
                    g.meta[order].status = CardStatus::ChopMoved;
                    g.meta[order].by = Some(giver);
                    g.meta[order].reason(turn);
                }
                StableResult::Stable(ClueInterp::Lock, g)
            }
        }

        _ => {
            let focus = *new_touched
                .iter()
                .max()
                .expect("clue newly touched nothing");
            let focus_pos = hand
                .iter()
                .position(|&o| o == focus)
                .expect("focus is in the hand");
            let target_index = hand
                .iter()
                .enumerate()
                .position(|(i, &o)| i > focus_pos && !state.deck[o].clued)
                .expect("no card to discard");
            let promised_order = list
                .iter()
                .copied()
                .filter(|&o| o > hand[target_index])
                .min()
                .expect("no promised card");

            log_info!(
                "ref discard on {}'s slot {}",
                state.names[receiver],
                target_index + 1
            );

            let mut g = game.clone();
            if state.variant.pinkish {
                g.common.update_thought(promised_order, |t| {
                    t.inferred = t.inferred.filter(|i| i.rank == clue.value)
                });
                g.meta[promised_order].focused = true;
                // Breaks pink promise
                if breaks_pink_promise(promised_order) {
                    return pink_failure();
                }
            } else {
                g.meta[focus].focused = true;
            }

            let meta = &mut g.meta[hand[target_index]];
            meta.status = CardStatus::CalledToDiscard;
            meta.by = Some(giver);
            meta.reason(turn);
            meta.signal(turn);

            StableResult::Stable(ClueInterp::Discard, g)
        }
    }
}
