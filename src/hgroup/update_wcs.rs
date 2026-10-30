//! Updating waiting connections as players act.

use super::conn_helper::assign_conns;
use super::{FStatus, HGroupGame, level};
use crate::basics::action::{Action, ClueAction, PlayAction};
use crate::basics::card::CardStatus;
use crate::basics::connection::{Connection, FinesseKind, FocusPossibility, WaitingConnection};
use crate::basics::identity_set::IdentitySetOpt;
use crate::basics::interp::{ClueInterp, Interp};
use crate::basics::{Game, Identifiable, Identity, IdentitySet};
use crate::logger::{LevelGuard, LogLevel};
use crate::util::players_until;
use crate::{log_error, log_highlight, log_info, log_warn};

#[derive(Clone, Debug)]
pub enum UpdateResult {
    Remove,
    Complete,
    Keep,
    Advance {
        next_index: usize,
        skipped: bool,
    },
    Demonstrated {
        next_index: Option<usize>,
        skipped: bool,
    },
    AmbiguousPassback,
    SelfPassback,
}

pub fn check_reveal_layer(prev: &Game, game: &Game, action: &ClueAction) -> Option<Game> {
    let state = &game.state;

    let revealed = state.hands[action.target].iter().copied().find(|&o| {
        let status = prev.meta[o].status;

        (!prev.state.deck[o].clued
            && matches!(status, CardStatus::Finessed | CardStatus::GentlemansDiscard))
            && !prev.common.thoughts[o].reset
            && game.common.thoughts[o].reset
    })?;

    // If revealed a layered finesse, replay with this future knowledge
    log_highlight!(
        crate::logger::CYAN,
        "revealed additional information about {revealed}! (expected {})",
        prev.common.str_infs(state, revealed)
    );
    let mut copy = game.clone();
    copy.future[revealed] = game.common.thoughts[revealed].possible;
    copy.replay(state.deck[revealed].turn_drawn).ok()
}

fn stomped_wc(prev: &Game, game: &Game, action: &Action, wc: &WaitingConnection) -> bool {
    match action {
        Action::Clue(clue_action) if wc.target != clue_action.giver => {
            let focus = game.determine_focus(prev, clue_action).focus;

            wc.connections.iter().any(|conn| {
                !(conn.hidden() && conn.reacting() == clue_action.giver) // Allow a hidden player to stomp, since they don't know
                    && conn
                        .ids()
                        .iter()
                        .all(|&i| game.state.deck[focus].id() == Some(i))
            })
        }
        _ => false,
    }
}

fn unplayable_alt<'a>(
    game: &'a Game,
    action: &Action,
    wc: &WaitingConnection,
) -> Option<&'a WaitingConnection> {
    let (order, reacting) = (wc.curr_conn().order(), wc.curr_conn().reacting());

    game.waiting().iter().find(|w| {
        *w != wc
            && w.focus == wc.focus
            // We can't defer if we know that this finesse is symmetric
            && !(w.symmetric && reacting == game.state.our_player_index)
            // We can only allow a defer if the reacting player has to wait for someone else, or they already tried to play
            && (w.curr_conn().reacting() != reacting || matches!(action, Action::Play(_)))
            && w.connections.iter().any(|conn| {
                conn.order() == order
                    && conn.ids().iter().any(|&i| {
                        !game.state.is_playable(i)
                            || matches!(action, Action::Play(PlayAction { id: Some(played), .. })
                                if i.prev() == Some(*played))
                    })
            })
    })
}

pub fn revert(g: &Game, order: usize, ids: &[Identity], force_reset: bool) -> Game {
    let id_set: IdentitySet = ids.iter().copied().collect();
    let new_inferred = g.common.thoughts[order].inferred.difference(id_set);

    let mut game = g.clone();
    if new_inferred.is_empty() || force_reset {
        game.common.update_thought(order, |t| {
            t.inferred = t
                .old_inferred
                .get()
                .map_or(t.possible, |old| old.intersect(t.possible));
            t.old_inferred = IdentitySetOpt::NONE;
            t.info_lock = IdentitySetOpt::NONE;
        });
        game.meta[order].status = CardStatus::None;
        game.meta[order].hidden = false;
    } else {
        game.common
            .update_thought(order, |t| t.inferred = new_inferred);
    }
    game
}

/// Plays the card in a hypothetical game, resolving waiting connections afterwards.
pub fn play_in_hypo(acc: &Game, order: usize, id: Identity, giver: usize) -> Game {
    let _quiet = LevelGuard::at_most(LogLevel::Error);
    let action = PlayAction {
        player_index: acc.state.holder_of[order],
        order,
        id: Some(id),
    };

    let mut after = acc.clone();
    after.on_play(&action);
    refresh_wcs(
        acc,
        &after,
        &Action::Play(action),
        false,
        true,
        Some(giver as isize),
    )
}

#[derive(Default)]
struct Struct {
    wcs: Vec<WaitingConnection>,
    to_remove: Vec<Connection>,
    rem_focuses: Vec<usize>,
    demos: Vec<WaitingConnection>,
}

pub fn refresh_wcs(
    prev: &Game,
    game: &Game,
    action: &Action,
    before_clue_interp: bool,
    elim: bool,
    hypo: Option<isize>,
) -> Game {
    if game.waiting().is_empty() {
        return game.clone();
    }

    let mut new_game = game.clone();
    new_game.hg_mut().waiting = Vec::new();
    let mut st = Struct::default();

    for wc in game.waiting() {
        let conns = &wc.connections;

        // Do not allow the hypo'ing player to resolve wcs on themselves
        // Do not refresh wcs that were just generated this turn
        let res = if hypo == Some(wc.target as isize) || wc.turn == game.state.turn_count {
            UpdateResult::Keep
        } else {
            update_wc(prev, game, action, wc, &st.to_remove, before_clue_interp)
        };

        match res {
            UpdateResult::Keep => st.wcs.push(wc.clone()),

            UpdateResult::Advance {
                next_index,
                skipped,
            } => {
                let mut kept = wc.clone();
                kept.connections = conns[next_index.min(conns.len())..].to_vec();
                st.wcs.push(kept);
                // (As in the Scala bot, when not skipping this appends the existing list to itself.)
                if !skipped {
                    let dup = st.to_remove.clone();
                    st.to_remove.extend(dup);
                } else {
                    st.to_remove
                        .extend(conns[..next_index.min(conns.len())].iter().cloned());
                }
            }

            UpdateResult::AmbiguousPassback => {
                let mut kept = wc.clone();
                kept.ambiguous_passback = true;
                st.wcs.push(kept);
            }

            UpdateResult::SelfPassback => {
                let mut kept = wc.clone();
                kept.self_passback = true;
                st.wcs.push(kept);
            }

            UpdateResult::Demonstrated {
                next_index,
                skipped,
            } => {
                let is_bluff = wc.curr_conn().is_bluff();
                let rest = |i: usize| {
                    let mut w = wc.clone();
                    w.connections = conns[i.min(conns.len())..].to_vec();
                    w
                };

                if !is_bluff && let Some(i) = next_index {
                    st.wcs.push(rest(i));
                }

                if !skipped {
                    let dup = st.to_remove.clone();
                    st.to_remove.extend(dup);
                } else {
                    match next_index {
                        None => st.to_remove.extend(conns.iter().cloned()),
                        Some(i) => st
                            .to_remove
                            .extend(conns[..i.min(conns.len())].iter().cloned()),
                    }
                }

                st.demos.push(match next_index {
                    None => wc.clone(),
                    Some(i) => rest(i),
                });
            }

            UpdateResult::Remove => {
                if hypo.is_none() {
                    st.to_remove.push(Connection::Prompt {
                        reacting: usize::MAX,
                        order: wc.focus,
                        id: wc.inference,
                        hidden: false,
                    });
                    if !(wc.symmetric || wc.ambiguous_self) {
                        st.to_remove.extend(conns.iter().cloned());
                    }
                    if !st.rem_focuses.contains(&wc.focus) {
                        st.rem_focuses.push(wc.focus);
                    }
                }
            }

            UpdateResult::Complete => {}
        }
    }

    let Struct {
        wcs: new_wcs,
        to_remove,
        rem_focuses,
        demos,
    } = st;

    let mut demo_ids: Vec<(usize, IdentitySet)> = Vec::new();
    for wc in &demos {
        match demo_ids.iter_mut().find(|(o, _)| *o == wc.focus) {
            Some((_, ids)) => *ids = ids.union(IdentitySet::from(wc.inference)),
            None => demo_ids.push((wc.focus, IdentitySet::from(wc.inference))),
        }
    }

    let (retain_wcs, non_demoed_wcs): (Vec<WaitingConnection>, Vec<WaitingConnection>) = new_wcs
        .into_iter()
        .partition(|wc| !demo_ids.iter().any(|(o, _)| *o == wc.focus) || demos.contains(wc));

    let mut g = new_game;

    for (order, ids) in &demo_ids {
        g.common.update_thought(*order, |t| {
            let new_inferred = t.inferred.intersect(*ids);
            t.inferred = new_inferred;
            t.info_lock = new_inferred.into();
        });
        if !g.xmeta(*order).f_status.is_empty() {
            g.xmeta_mut(*order).f_status = Vec::new();
        }
    }

    for wc in &demos {
        if wc.ambiguous_self {
            continue;
        }

        if wc.connections[0].hidden() {
            // Demonstrating a hidden connection means they must play all the hidden cards + the actual one
            let non_hidden_index = wc.connections.iter().position(|c| !c.hidden());
            if let Some(nhi) = non_hidden_index {
                for i in 1..=nhi {
                    g.xmeta_mut(wc.connections[i].order()).f_status = Vec::new();
                }
            }
        }

        // Check if PossiblyOn status needs to be removed
        for conn in &wc.connections {
            if g.xmeta(conn.order()).f_status.is_empty() {
                continue;
            }

            let still_maybe_finessed = retain_wcs.iter().any(|w| {
                w.focus == wc.focus
                    && w.turn == wc.turn
                    && !w
                        .connections
                        .iter()
                        .any(|c| matches!(c, Connection::Finesse { order, .. } if *order == conn.order()))
            });

            if !still_maybe_finessed {
                log_highlight!(
                    crate::logger::CYAN,
                    "removing PossiblyOn from {} after demonstration!",
                    conn.order()
                );
                g.xmeta_mut(conn.order())
                    .f_status
                    .retain(|f| !matches!(f, FStatus::PossiblyOn(_)));
            }
        }
    }

    let all_remove: Vec<Connection> = to_remove
        .into_iter()
        .chain(
            non_demoed_wcs
                .iter()
                .filter(|wc| !(wc.symmetric || wc.ambiguous_self))
                .flat_map(|wc| wc.connections.iter().cloned()),
        )
        .collect();

    for conn in all_remove {
        match &conn {
            Connection::Positional { ambiguous_own, .. } => {
                if let Some((order, poss)) = ambiguous_own
                    && g.meta[*order].status != CardStatus::CalledToPlay
                    && !g.common.thoughts[*order]
                        .possible
                        .intersect(g.state.playable_set)
                        .is_empty()
                {
                    log_highlight!(crate::logger::CYAN, "rewriting onto us! {order}");
                    let poss: IdentitySet = poss.iter().copied().collect();
                    g.common.update_thought(*order, |t| {
                        t.old_inferred = t.inferred.into();
                        t.inferred = t.inferred.intersect(poss);
                    });
                    g.meta[*order].status = CardStatus::CalledToPlay;
                    g.meta[*order].focused = true;
                }
            }
            _ => {
                let shared = !conn.is_bluff()
                    && retain_wcs.iter().any(|wc| {
                        (wc.focus == conn.order() && conn.ids().iter().all(|&i| i == wc.inference))
                            || wc.connections.iter().any(|c| {
                                c.order() == conn.order()
                                    && c.ids().len() == conn.ids().len()
                                    && c.ids().iter().all(|i| conn.ids().contains(i))
                            })
                    });

                if shared || matches!(conn, Connection::Known { .. } | Connection::Playable { .. })
                {
                    continue;
                }
                g = revert(
                    &g,
                    conn.order(),
                    conn.ids(),
                    matches!(
                        conn,
                        Connection::Finesse {
                            kind: FinesseKind::Bluff,
                            ..
                        }
                    ),
                );
            }
        }
    }

    let mut disambiguated: Vec<WaitingConnection> = Vec::new();
    for focus in rem_focuses {
        let same_focus: Vec<&WaitingConnection> =
            retain_wcs.iter().filter(|wc| wc.focus == focus).collect();

        if !same_focus.is_empty()
            && same_focus
                .iter()
                .all(|wc| wc.symmetric || wc.ambiguous_self)
        {
            let turn = same_focus[0].turn;
            let clue_action = g.state.action_list[turn]
                .iter()
                .find_map(|a| match a {
                    Action::Clue(c) => Some(c.clone()),
                    _ => None,
                })
                .expect("a waiting connection from a turn without a clue");

            // TODO(Scala): Only write the next-simplest connections, not all of them?
            let ambiguous_wcs: Vec<&WaitingConnection> = same_focus
                .iter()
                .copied()
                .filter(|wc| wc.ambiguous_self)
                .collect();

            log_error!(
                "assigning previously-ambiguous connections on {focus} to {:?}",
                ambiguous_wcs
                    .iter()
                    .map(|w| game.state.log_id(w.inference))
                    .collect::<Vec<_>>()
            );

            let fps: Vec<FocusPossibility> = ambiguous_wcs
                .iter()
                .map(|wc| {
                    let mut fp = FocusPossibility::new(
                        wc.inference,
                        wc.connections.clone(),
                        Interp::Clue(ClueInterp::Play),
                    );
                    fp.symmetric = wc.symmetric;
                    fp
                })
                .collect();

            let mut g2 = assign_conns(&g, &clue_action, &fps, focus, &[], true);
            let inferences: IdentitySet = ambiguous_wcs.iter().map(|w| w.inference).collect();
            g2.common
                .update_thought(focus, |t| t.inferred = t.possible.intersect(inferences));

            disambiguated.extend(ambiguous_wcs.into_iter().cloned());
            g = g2;
        }
    }

    g.hg_mut().waiting = retain_wcs
        .into_iter()
        .map(|mut w| {
            if disambiguated.contains(&w) {
                w.ambiguous_self = false;
            }
            w
        })
        .collect();

    if elim {
        let except = hypo.filter(|&h| h >= 0).map(|h| h as usize);
        g.elim(except, hypo.is_some());
    }
    g
}

pub fn update_wc(
    prev: &Game,
    game: &Game,
    action: &Action,
    wc: &WaitingConnection,
    to_remove: &[Connection],
    before_clue_interp: bool,
) -> UpdateResult {
    if wc.connections.is_empty() {
        return UpdateResult::Complete;
    }

    let state = &game.state;
    let reacting = wc.curr_conn().reacting();

    if !before_clue_interp {
        log_info!(
            "waiting for connecting {} ({}) {} for {} ({}){}{}",
            wc.curr_conn()
                .ids()
                .iter()
                .map(|&i| state.log_id(i))
                .collect::<Vec<_>>()
                .join(","),
            wc.curr_conn().order(),
            wc.curr_conn().kind(),
            state.log_id(wc.inference),
            wc.focus,
            if wc.symmetric { " (symmetric)" } else { "" },
            if wc.ambiguous_self {
                " (ambiguous self)"
            } else {
                ""
            }
        );
    }

    let impossible_conn = wc.connections.iter().find(|conn| match conn {
        // For playable conns, all linked cards must be impossible
        Connection::Playable { linked, id, .. } if !linked.is_empty() => linked.iter().all(|&l| {
            game.common.thoughts[l]
                .possible
                .intersect(IdentitySet::from(*id))
                .is_empty()
        }),
        Connection::Finesse {
            order,
            ids,
            inverted: true,
            ..
        } => {
            let ids: IdentitySet = ids.iter().copied().collect();
            game.common.thoughts[*order]
                .possible
                .intersect(state.trash_set.union(ids))
                .is_empty()
        }
        Connection::Finesse {
            order,
            kind: FinesseKind::Bluff,
            ..
        } => game.common.thoughts[*order]
            .possible
            .intersect(prev.state.playable_set)
            .is_empty(),
        conn => {
            let ids: IdentitySet = conn.ids().iter().copied().collect();
            game.common.thoughts[conn.order()]
                .possible
                .intersect(ids)
                .is_empty()
        }
    });

    let impossible_focus = !matches!(wc.connections.first(), Some(Connection::Positional { .. }))
        && !game.common.thoughts[wc.focus]
            .possible
            .contains(wc.inference);

    if !prev.state.hands[reacting].contains(&wc.curr_conn().order()) {
        // Extra wc that wasn't removed
        log_warn!(
            "removing extra wc {} ({}) {:?} {}",
            state.log_id(wc.inference),
            wc.curr_conn().order(),
            prev.state.hands[reacting],
            state.names[reacting]
        );
        UpdateResult::Remove
    } else if let Some(c) = impossible_conn {
        log_warn!(
            "future connection depends on {} having ids {}, removing",
            c.order(),
            c.ids()
                .iter()
                .map(|&i| state.log_id(i))
                .collect::<Vec<_>>()
                .join(",")
        );
        UpdateResult::Remove
    } else if impossible_focus {
        log_warn!(
            "connection depends on focus {} having id {}, removing",
            wc.focus,
            state.log_id(wc.inference)
        );
        UpdateResult::Remove
    } else if stomped_wc(prev, game, action, wc) {
        log_warn!("connection was clued directly, cancelling");
        UpdateResult::Remove
    } else if (wc.ambiguous_self && wc.curr_conn().reacting() == state.our_player_index)
        || before_clue_interp
    {
        UpdateResult::Keep
    } else if other_play(game, action, wc) {
        log_highlight!(
            crate::logger::CYAN,
            "{} played connecting card, continuing",
            state.names[action.player_index().unwrap_or(0)]
        );
        resolve_other_play(game, wc)
    }
    // The turn we were waiting for
    else if action.player_index() == Some(reacting) {
        if state.hands[reacting].contains(&wc.curr_conn().order()) {
            let curr = wc.curr_conn();

            if to_remove.iter().any(|r| {
                r.order() == curr.order() && r.ids().iter().all(|i| curr.ids().contains(i))
            }) {
                log_highlight!(
                    crate::logger::CYAN,
                    "previously removed {} ({}), removing",
                    curr.order(),
                    curr.ids()
                        .iter()
                        .map(|&i| state.log_id(i))
                        .collect::<String>()
                );
                UpdateResult::Remove
            } else if let Some(alt) = unplayable_alt(game, action, wc) {
                log_highlight!(
                    crate::logger::CYAN,
                    "not all possibilities playable {}, waiting",
                    state.log_conns(&alt.connections)
                );
                UpdateResult::Keep
            } else {
                resolve_retained(prev, game, action, wc)
            }
        } else {
            match action {
                Action::Play(p) => resolve_played(game, wc, p.order),

                Action::Discard(d)
                    if d.failed
                        || matches!(wc.curr_conn(), Connection::Finesse { inverted: true, .. }) =>
                {
                    resolve_played(game, wc, d.order)
                }

                Action::Discard(d) => {
                    log_highlight!(
                        crate::logger::YELLOW,
                        "waiting card {} discarded?? {:?}",
                        state.log_order(wc.curr_conn().order()),
                        prev.chop(d.player_index)
                    );

                    if prev.chop(d.player_index) == Some(d.order)
                        && !matches!(
                            wc.curr_conn(),
                            Connection::Finesse {
                                kind: FinesseKind::Certain,
                                ..
                            }
                        )
                    {
                        UpdateResult::Remove
                    } else {
                        // Will be updated when the dc is interpreted
                        UpdateResult::Keep
                    }
                }

                _ => panic!("impossible"),
            }
        }
    } else {
        UpdateResult::Keep
    }
}

fn other_play(game: &Game, action: &Action, wc: &WaitingConnection) -> bool {
    let thought = &game.me().thoughts[wc.curr_conn().order()];

    let ids = wc.curr_conn().ids();

    // The card needs to match our thoughts as well as the hypothesized identity in the connection
    matches!(action, Action::Play(PlayAction { order, id: Some(id), .. })
        if *order != wc.curr_conn().order()
            && thought.matches_with(id, true, false, true)
            && ids.len() == 1
            && ids.contains(id))
        && !wc.curr_conn().hidden() // Don't advance if the real connection is layered, because that player won't skip
        && !wc.curr_conn().is_bluff() // Someone else can't play into a bluff
}

fn resolve_other_play(game: &Game, wc: &WaitingConnection) -> UpdateResult {
    let mut tail = wc.clone();
    tail.connections = wc.connections[1..].to_vec();
    let next_index = tail.next_index(&game.state);

    if matches!(wc.curr_conn(), Connection::Finesse { .. }) {
        UpdateResult::Demonstrated {
            next_index: next_index.map(|i| i + 1),
            skipped: true,
        }
    } else {
        next_index.map_or(UpdateResult::Complete, |i| UpdateResult::Advance {
            next_index: i + 1,
            skipped: true,
        })
    }
}

fn resolve_retained(
    prev: &Game,
    game: &Game,
    action: &Action,
    wc: &WaitingConnection,
) -> UpdateResult {
    let state = &game.state;
    let (reacting, conn_order) = (wc.curr_conn().reacting(), wc.curr_conn().order());
    let name = &state.names[reacting];

    let new_finesse_queued = wc.connections.iter().find(|conn| {
        matches!(conn, Connection::Finesse { .. })
            && state.hands[conn.reacting()].iter().any(|&o| {
                // A newer finesse exists on this player that is not part of the same suit.
                game.is_blind_playing(o)
                    && game.xmeta(o).turn_finessed.is_some_and(|t| {
                        game.xmeta(conn.order())
                            .turn_finessed
                            .is_some_and(|t2| t > t2)
                    })
                    && !game.common.thoughts[o]
                        .inferred
                        .iter()
                        .any(|i| i.suit_index == conn.ids()[0].suit_index)
            })
    });

    let unplayable_wcs: Vec<&WaitingConnection> = prev
        .waiting()
        .iter()
        .filter(|w| {
            w.turn <= wc.turn
                && w.connections.iter().any(|conn| {
                    conn.order() == conn_order
                        && conn
                            .ids()
                            .iter()
                            .any(|&i| state.is_useful(i) && !prev.state.is_playable(i))
                })
        })
        .collect();

    if !unplayable_wcs.is_empty() {
        log_info!(
            "{name} didn't play into unplayable {}",
            wc.curr_conn().kind()
        );
        return UpdateResult::Keep;
    }

    let hidden_wc = game.waiting().iter().find(|w| {
        // This is a hidden connection that is not currently revealed
        !w.connections.is_empty()
            && w.connections[1..]
                .iter()
                .any(|c| c.order() == conn_order && c.hidden())
    });

    if let Some(h) = hidden_wc {
        log_info!(
            "hidden connection as part of {}",
            state.log_conns(&h.connections)
        );
        return UpdateResult::Keep;
    }

    let missed_reaction = new_finesse_queued.is_some()
        || match wc.curr_conn() {
            Connection::Known { order, .. } => game.is_blind_playing(*order),
            Connection::Finesse { .. } | Connection::Positional { .. } => true,
            Connection::Prompt { .. } => match &game.last_actions[reacting] {
                Some(Action::Clue(_)) => game.last_move() == Some(ClueInterp::Stall.into()),
                Some(Action::Discard(_)) => true,
                _ => false,
            },
            Connection::Playable { .. } => false,
        };

    if !missed_reaction {
        return UpdateResult::Keep;
    }

    let older_unplayable_finesse = game.level() >= level::INTERMEDIATE_FINESSES
        && matches!(wc.curr_conn(), Connection::Finesse { .. })
        && state.hands[reacting].iter().enumerate().any(|(i, &o)| {
            o < conn_order
                && !(state.deck[o].clued
                    || state.hands[reacting][i..]
                        .iter()
                        .all(|&h| state.deck[h].clued))
                && game.is_blind_playing(o)
                && game.xmeta(o).turn_finessed.is_some_and(|t| t < wc.turn)
                && game.common.thoughts[o]
                    .inferred
                    .iter()
                    .any(|i| state.is_useful(i) && !state.is_playable(i))
        });

    if older_unplayable_finesse {
        log_info!("allowing {name} to clue, need to wait for unplayable older finesse");
        return UpdateResult::Keep;
    }

    match action {
        Action::Clue(_) => {
            if matches!(wc.curr_conn(), Connection::Prompt { .. })
                && game.last_move().is_some_and(|m| {
                    m != ClueInterp::Mistake.into() && m != ClueInterp::Stall.into()
                })
            {
                log_info!(
                    "allowing {name} to defer a prompt by giving a useful clue {:?}",
                    game.last_move()
                );
                return UpdateResult::Keep;
            }

            if game.level() >= level::INTERMEDIATE_FINESSES && game.important_action(reacting) {
                if wc.curr_conn().is_possibly_bluff() {
                    log_highlight!(
                        crate::logger::CYAN,
                        "{name} not allowed to defer a potential bluff"
                    );
                    return UpdateResult::Remove;
                }
                log_info!("allowing {name} to defer a finesse for an important clue");
                return UpdateResult::Keep;
            }

            if matches!(wc.curr_conn(), Connection::Positional { .. }) {
                log_info!("allowing {name} to defer a positional by cluing!");
                return UpdateResult::Keep;
            }
        }

        Action::Play(PlayAction { order, .. }) => {
            if game.is_blind_playing(*order) {
                if game
                    .xmeta(*order)
                    .turn_finessed
                    .is_some_and(|t| t < wc.turn)
                {
                    log_info!("{name} played into older finesse, continuing to wait");
                    return UpdateResult::Keep;
                }

                if game.meta[conn_order].hidden && state.deck[conn_order].clued {
                    log_info!("{name} jumped ahead in layered finesse, continuing to wait");
                    return UpdateResult::Keep;
                }
            }
        }

        Action::Discard(d)
            if matches!(wc.curr_conn(), Connection::Positional { .. })
                && prev.chop(d.player_index) != Some(d.order) =>
        {
            log_info!("allowing {name} to defer a positional by performing a positional!");
            return UpdateResult::Keep;
        }

        _ => {}
    }

    let passback = match wc.curr_conn() {
        Connection::Finesse {
            kind,
            ids,
            ambiguous_passback,
            ..
        } => {
            let non_hidden_conns = wc
                .connections
                .iter()
                .filter(|conn| {
                    !conn.hidden()
                        && !state.deck[conn.order()].clued
                        && conn.reacting() == wc.curr_conn().reacting()
                })
                .count() as isize;

            *kind != FinesseKind::Bluff
                && reacting != state.our_player_index // can't pass back to ourselves
                && non_hidden_conns > 1 // they need to play more than 1 card
                && non_hidden_conns
                    == wc.inference.rank as isize
                        - state.play_stacks[wc.inference.suit_index] as isize
                        - 1 // they have all required cards
                && !ids.iter().all(|&i| state.is_critical(i)) // the id is not critical
                && !ambiguous_passback
        }
        _ => false,
    };

    if passback {
        log_info!(
            "{name} didn't play into {} but they need to play multiple non-hidden cards, passing back",
            wc.curr_conn().kind()
        );
        return UpdateResult::AmbiguousPassback;
    }

    let allowable_hesitation = if reacting != wc.target || wc.curr_conn().is_bluff() {
        None
    } else {
        // Find all waiting connections using this order OR self-identities, and merge their possible identities
        let linked_ids: Vec<Identity> = game
            .waiting()
            .iter()
            .filter(|w| *w != wc && w.focus == wc.focus)
            .flat_map(|w| {
                w.connections
                    .iter()
                    .filter(|c| c.order() == conn_order || c.reacting() == w.target)
                    .flat_map(|c| c.ids().iter().copied())
            })
            .collect();

        players_until(
            state.num_players,
            state.next_player_index(reacting),
            wc.giver,
        )
        .find(|&pi| {
            game.find_finesse(pi, Default::default(), Default::default())
                .is_some_and(|finesse| {
                    linked_ids
                        .iter()
                        .any(|&i| game.common.thoughts[finesse].inferred.contains(i))
                })
        })
    };

    if let Some(pi) = allowable_hesitation {
        log_highlight!(
            crate::logger::CYAN,
            "{} didn't play but allowable hesitation on {}",
            state.names[reacting],
            state.names[pi]
        );
        return UpdateResult::SelfPassback;
    }

    if let Some(other_wc) = game.potential_clandestine_wc(reacting, conn_order, wc) {
        log_highlight!(
            crate::logger::CYAN,
            "{} didn't play but potential clandestine finesse {}",
            state.names[reacting],
            state.log_conns_to(&other_wc.connections, other_wc.inference)
        );
        return UpdateResult::Keep;
    }

    let clued_finesse = matches!(wc.curr_conn(), Connection::Finesse { .. })
        && state.deck[conn_order].clued
        && game.common.thoughts[conn_order]
            .inferred
            .iter()
            .all(|i| state.is_playable(i));

    if clued_finesse {
        log_highlight!(
            crate::logger::CYAN,
            "{} didn't play but clued and known playable!",
            state.names[reacting]
        );
        return UpdateResult::Keep;
    }

    log_highlight!(
        crate::logger::CYAN,
        "{name} didn't react with a waiting connection, removing {}",
        state.log_id(wc.inference)
    );
    UpdateResult::Remove
}

fn resolve_played(game: &Game, wc: &WaitingConnection, play: usize) -> UpdateResult {
    let state = &game.state;

    log_info!("waiting card {} played", state.log_order(play));

    let next_index = wc.next_index(state);

    let missed_reaction = matches!(
        wc.curr_conn(),
        Connection::Finesse { .. } | Connection::Prompt { .. }
    );

    let stomped_finesse = || {
        let order = wc.curr_conn().order();
        let card = &state.deck[order];

        card.clued
            && card.clues.last().is_some_and(|c| c.turn > wc.turn)
            && (game.meta[order].focused
                || (game
                    .common
                    .thinks_playables(game, wc.curr_conn().reacting(), false, true)
                    .is_empty()
                    // (`oldPossible.get` throws in Scala when there is none.)
                    && game.common.thoughts[order]
                        .old_possible
                        .get()
                        .unwrap_or(game.common.thoughts[order].possible)
                        .iter()
                        .all(|i| state.is_playable(i) || card.matches_assume(&i) || state.is_basic_trash(i))))
    };

    if missed_reaction {
        if matches!(wc.curr_conn(), Connection::Finesse { .. }) && stomped_finesse() {
            log_info!("connecting card was stomped on, not confirming finesse");

            if next_index.is_some_and(|i| wc.connections[i].reacting() == wc.curr_conn().reacting())
            {
                log_info!("connection requires a self blind play, removing due to occam's razor");
                return UpdateResult::Remove;
            }
        } else if matches!(
            wc.curr_conn(),
            Connection::Prompt { .. } | Connection::Finesse { .. }
        ) || game.level() < level::INTERMEDIATE_FINESSES
        {
            return UpdateResult::Demonstrated {
                next_index,
                skipped: false,
            };
        }
    }

    next_index.map_or(UpdateResult::Complete, |i| UpdateResult::Advance {
        next_index: i,
        skipped: false,
    })
}
