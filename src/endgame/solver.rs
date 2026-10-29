use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long a new solver may search, in milliseconds (3 s, as in the Scala bot, unless [`set_default_timeout`] says otherwise).
static DEFAULT_TIMEOUT_MS: AtomicU64 = AtomicU64::new(3000);

/// Changes the time limit of solvers created from now on (process-wide).
pub fn set_default_timeout(timeout: Duration) {
    DEFAULT_TIMEOUT_MS.store(timeout.as_millis() as u64, Ordering::Relaxed);
}

use super::helper::{GameArr, gen_arrs, trivially_winnable, unwinnable_state};
use super::winnable::{SimpleResult, clueless_winnable, winnable_if};
use crate::basics::{Action, CardStatus, Game, Identifiable, Identity, PerformAction, State};
use crate::collections::FastBitSet;
use crate::frac::Frac;
use crate::logger::{self, LevelGuard, LogLevel};
use crate::util::{VisibleOpts, indent, visible_find};
use crate::{log_highlight, log_info};

/// `Err` carries the reason a game isn't winnable (empty if there is nothing interesting to say).
pub type WinnableResult = Result<(Vec<PerformAction>, Frac), &'static str>;

const UNWINNABLE: &str = "";
const TIMEOUT: &str = "timeout";

/// How many copies of each identity are still unseen, indexed by identity ordinal.
///
/// The Scala bot used an immutable `Map[Identity, Int]`, whose iteration order is unspecified once it
/// has more than four entries. This iterates by ordinal, which makes the solver deterministic.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct RemainingMap([u8; 30]);

impl RemainingMap {
    pub fn get(&self, id: Identity) -> usize {
        usize::from(self.0[id.to_ord()])
    }

    pub fn set(&mut self, id: Identity, missing: usize) {
        self.0[id.to_ord()] = missing as u8;
    }

    /// Whether no identities remain.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&m| m == 0)
    }

    /// The total number of cards remaining.
    pub fn total(&self) -> usize {
        self.0.iter().map(|&m| usize::from(m)).sum()
    }

    /// The identities remaining (with how many copies are missing), by ordinal.
    pub fn iter(&self) -> impl Iterator<Item = (Identity, usize)> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter(|&(_, &m)| m > 0)
            .map(|(ord, &m)| (Identity::from_ord(ord), usize::from(m)))
    }

    pub fn first(&self) -> Option<(Identity, usize)> {
        self.iter().next()
    }

    /// The remaining cards after one copy of `id` is removed.
    ///
    /// # Panics
    /// If `id` is not remaining.
    pub fn rem(&self, id: Identity) -> Self {
        let mut new = *self;
        assert!(new.0[id.to_ord()] > 0, "{id:?} is not remaining");
        new.0[id.to_ord()] -= 1;
        new
    }

    pub fn fmt2(&self, state: &State) -> String {
        self.iter()
            .map(|(id, missing)| format!("{} (missing {missing})", state.log_id(id)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The identities we can't see (with how many copies are missing), and our own cards
/// (most recently drawn last-to-first, with their identity if known).
pub fn find_remaining_ids(game: &Game) -> (RemainingMap, Vec<(usize, Option<Identity>)>) {
    let state = &game.state;
    let mut seen = RemainingMap::default();
    let mut own_ids = Vec::new();

    for (i, hand) in state.hands.iter().enumerate() {
        for &order in hand {
            // Identify all the cards we know for sure
            match game.me().thoughts[order].id() {
                Some(id) => {
                    seen.set(id, seen.get(id) + 1);
                    if i == state.our_player_index {
                        own_ids.push((order, Some(id)));
                    }
                }
                None if i == state.our_player_index => own_ids.push((order, None)),
                None => {}
            }
        }
    }
    // The Scala bot prepends as it goes.
    own_ids.reverse();

    let mut remaining = RemainingMap::default();
    for id in state.variant.all_ids() {
        let ord = id.to_ord();
        let missing =
            state.card_count[ord] as isize - state.base_count[ord] as isize - seen.get(id) as isize;
        if missing > 0 {
            remaining.set(id, missing as usize);
        }
    }

    (remaining, own_ids)
}

/// A possible assignment of identities to our unknown cards.
#[derive(Clone, Debug)]
struct Arrangement {
    ids: Vec<Identity>,
    prob: Frac,
    remaining: RemainingMap,
}

type Actions = Vec<(PerformAction, Vec<Identity>)>;

/// A hypothetical game for one arrangement of our hand.
struct Hypo {
    game: Game,
    actions: Actions,
    arrs: (Vec<GameArr>, Vec<GameArr>),
    prob: Frac,
    /// The identities we can't see in this arrangement.
    remaining: RemainingMap,
}

#[derive(Debug)]
pub struct EndgameSolver {
    /// Per search depth: the average winrate (and number of samples) of each action seen so far.
    /// Used to try the historically best actions first.
    pub success_rate: HashMap<usize, HashMap<PerformAction, (Frac, i64)>>,
    /// If true, only picks a few random permutations of trash instead of trying all of them.
    /// May make winrate slightly off.
    pub monte_carlo: bool,
    pub timeout: Duration,
}

impl Default for EndgameSolver {
    fn default() -> Self {
        Self::new(true)
    }
}

impl EndgameSolver {
    pub fn new(monte_carlo: bool) -> Self {
        Self {
            success_rate: HashMap::new(),
            monte_carlo,
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS.load(Ordering::Relaxed)),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Returns the action with the highest winrate by brute-forcing permutations of the deck.
    /// Will not solve if there are at least 4 unique ids we haven't seen yet.
    /// Terminates early and returns a partially-computed result if solving takes too long.
    ///
    /// * `only_action`: whether to only find the winrate for a particular action.
    ///
    /// Returns `Ok((best_action, winrate))` if a non-zero winrate is found, otherwise `Err(reason)`.
    pub fn solve(
        &mut self,
        game: &Game,
        only_action: Option<PerformAction>,
    ) -> Result<(PerformAction, Frac), String> {
        let state = &game.state;
        let our = state.our_player_index;

        if state.score() + 1 == state.max_score() {
            let playables = if game.good_touch() || state.endgame_turns.is_some() {
                game.me().thinks_playables(game, our, true, true)
            } else {
                game.me().obvious_playables(game, our)
            };

            let winning_play = playables.into_iter().find(|&o| {
                game.me().thoughts[o]
                    .id_with(true, false)
                    .is_some_and(|id| state.is_playable(id))
            });

            if let Some(order) = winning_play {
                return Ok((PerformAction::try_play(game, order), Frac::ONE));
            }
        }

        let start = Instant::now();
        let deadline = start + self.timeout;
        let (remaining_ids, own_ids) = find_remaining_ids(game);

        if remaining_ids
            .iter()
            .filter(|&(id, v)| state.is_useful(id) && v == state.card_count[id.to_ord()])
            .count()
            > 3
        {
            let missing_ids = remaining_ids
                .iter()
                .filter(|&(id, _)| state.is_useful(id))
                .map(|(id, _)| state.log_id(id))
                .collect::<Vec<_>>()
                .join(",");
            return Err(format!("couldn't find any {missing_ids}!"));
        }

        let _quiet = LevelGuard::set(LogLevel::Off);

        let mut assumed_game = game.clone();
        for &(order, id) in &own_ids {
            if let Some(id) = id {
                assumed_game.set_id(order, id);
            }
        }

        let linked_orders = game.me().linked_orders(state, false);
        let unknown_own: Vec<usize> = own_ids
            .iter()
            .filter(|(_, id)| id.is_none())
            .map(|&(order, _)| order)
            .collect();

        let total_unknown = state.cards_left + unknown_own.len();
        log_info!(
            "unknown own {unknown_own:?}, cards left {}",
            state.cards_left
        );

        if total_unknown == 0 {
            return match self.winnable(&assumed_game, our, &remaining_ids, deadline, 0) {
                Ok((actions, winrate)) if !actions.is_empty() => {
                    log_highlight!(
                        logger::MAGENTA,
                        "winnable! actions {}",
                        actions
                            .iter()
                            .map(|a| a.fmt(&assumed_game, None))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    log_info!("solved in {}ms", start.elapsed().as_millis());
                    Ok((actions[0], winrate))
                }
                Ok(_) => Err("timeout".to_string()),
                Err(_) => Err("couldn't find a winning strategy".to_string()),
            };
        }

        log_info!("remaining ids: {}", remaining_ids.fmt2(state));

        let impossible_arr = |ids: &[Identity], id: Identity, order: usize, try_filter: bool| {
            let thought = &game.me().thoughts[order];

            if state.deck[order].id().is_some_and(|i| i != id) || !thought.possible.contains(id) {
                return true;
            }

            if try_filter && !game.valid_arr(id, order) {
                log_info!(
                    "arranging: conventionally unlikely to assign {} to {order}, ignoring",
                    state.log_id(id)
                );
                return true;
            }

            // We cannot assign a trash id if it is linked for a non-trash id and all other orders are already trash
            if state.is_basic_trash(id) && linked_orders.contains(order) {
                let link = game.me().links.iter().find(|l| {
                    let orders = l.orders();
                    l.promise().is_some_and(|p| state.is_useful(p))
                        && orders.contains(order)
                        && orders.iter().all(|o| {
                            o == order
                                || ids.iter().enumerate().any(|(i, &id2)| {
                                    o == unknown_own[i] && state.is_basic_trash(id2)
                                })
                        })
                });

                if let Some(link) = link {
                    log_info!(
                        "arranging: {order} is linked to {} for {} but others are already trash, can't assign {}",
                        link.orders(),
                        link.promise().map_or_else(String::new, |p| state.log_id(p)),
                        state.log_id(id)
                    );
                    return true;
                }
            }

            false
        };

        let expand_arr = |arrangement: &Arrangement, try_filter: bool| -> Vec<Arrangement> {
            if Instant::now() > deadline {
                return vec![arrangement.clone()];
            }

            let Arrangement {
                ids,
                prob,
                remaining,
            } = arrangement;
            let total_cards = remaining.total() as i64;

            remaining
                .iter()
                .filter(|&(id, _)| !impossible_arr(ids, id, unknown_own[ids.len()], try_filter))
                .map(|(id, missing)| {
                    let mut new_ids = ids.clone();
                    new_ids.push(id);
                    Arrangement {
                        ids: new_ids,
                        prob: *prob * missing as i64 / total_cards,
                        remaining: remaining.rem(id),
                    }
                })
                .collect()
        };

        let expand_all = |try_filter: bool| -> Vec<Arrangement> {
            let mut layer = vec![Arrangement {
                ids: Vec::new(),
                prob: Frac::ONE,
                remaining: remaining_ids,
            }];
            for _ in 0..unknown_own.len() {
                layer = layer
                    .iter()
                    .flat_map(|a| expand_arr(a, try_filter))
                    .collect();
            }
            layer
        };

        let mut all_arrs = expand_all(true);
        if all_arrs.is_empty() {
            log_highlight!(logger::YELLOW, "trying again with no filter");
            all_arrs = expand_all(false);
        }

        if Instant::now() > deadline {
            return Err(TIMEOUT.to_string());
        }

        let mut arrs = if self.monte_carlo {
            // Accumulate matching non-trash arrangements into one probability.
            let mut sum_prob = Frac::ZERO;
            let mut index: HashMap<Vec<u8>, usize> = HashMap::new();
            let mut grouped: Vec<Arrangement> = Vec::new();

            for arr in all_arrs {
                let non_trash: Vec<u8> = arr
                    .ids
                    .iter()
                    .map(|&id| {
                        if state.is_basic_trash(id) {
                            u8::MAX
                        } else {
                            id.to_ord() as u8
                        }
                    })
                    .collect();

                sum_prob += arr.prob;
                match index.get(&non_trash) {
                    Some(&i) => grouped[i].prob += arr.prob,
                    None => {
                        index.insert(non_trash, grouped.len());
                        grouped.push(arr);
                    }
                }
            }

            // Normalize all probabilities: some of the potential generated ones may be impossible,
            // so the total prob may be less than 1.
            for arr in &mut grouped {
                arr.prob = arr.prob / sum_prob;
            }
            grouped
        } else {
            all_arrs
        };

        arrs.sort_by_key(|a| std::cmp::Reverse(a.prob));
        if arrs.is_empty() {
            arrs.push(Arrangement {
                ids: Vec::new(),
                prob: Frac::ONE,
                remaining: remaining_ids,
            });
        }

        log_info!(
            "arrangements {} (size {})",
            arrs.iter()
                .map(|a| a
                    .ids
                    .iter()
                    .map(|&i| state.log_id(i))
                    .collect::<Vec<_>>()
                    .join(","))
                .collect::<Vec<_>>()
                .join(" | "),
            arrs.len()
        );

        let hypos: Vec<Hypo> = arrs
            .into_iter()
            .map(
                |Arrangement {
                     ids,
                     prob,
                     remaining,
                 }| {
                    let mut hypo = assumed_game.clone();
                    for (i, &id) in ids.iter().enumerate() {
                        hypo.set_id(unknown_own[i], id);
                    }

                    let mut actions =
                        self.possible_actions(&hypo, our, &remaining, deadline, 0, false);
                    if actions.is_empty() {
                        actions = self.possible_actions(&hypo, our, &remaining, deadline, 0, true);
                    }

                    let arrs = gen_arrs(
                        &hypo,
                        &remaining,
                        our,
                        actions.iter().all(|a| a.0.is_clue()),
                    );

                    Hypo {
                        game: hypo,
                        actions,
                        arrs,
                        prob,
                        remaining,
                    }
                },
            )
            .collect();

        if let Some(only) = only_action {
            let mut winrate = Frac::ZERO;
            for hypo in &hypos {
                // `only` may not be a move the convention would consider (for example discarding another trash card than the
                // expected one); a move that isn't a candidate still has a winrate, so evaluate it anyway.
                let candidate = hypo
                    .actions
                    .iter()
                    .find(|a| a.0 == only)
                    .cloned()
                    .or_else(|| {
                        match winnable_if(&hypo.game.state, our, only, &hypo.remaining, deadline, 0)
                        {
                            SimpleResult::Unwinnable => None,
                            SimpleResult::WinnableWithDraws(draws) => Some((only, draws)),
                            SimpleResult::AlwaysWinnable => Some((only, Vec::new())),
                        }
                    });

                if let Some(perform) = candidate {
                    let (undrawn, drawn) = &hypo.arrs;
                    let arrs = if perform.0.is_clue() { undrawn } else { drawn };
                    winrate +=
                        hypo.prob * self.action_winrate(&hypo.game, arrs, &perform, our, deadline);
                }
            }
            return Ok((only, winrate));
        }

        let mut all_actions: Vec<PerformAction> = Vec::new();
        for hypo in &hypos {
            for (perform, _) in &hypo.actions {
                if !all_actions.contains(perform) {
                    all_actions.push(*perform);
                }
            }
        }

        let first = &hypos[0];

        log_info!(
            "all actions: {}",
            all_actions
                .iter()
                .map(|a| a.fmt(game, None))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut initial_actions: Vec<(PerformAction, Frac)> = {
            let init = self.optimize_full(
                &first.game,
                &first.arrs,
                &first.actions,
                our,
                deadline,
                hypos.len() == 1,
            );

            // Apply probability of first arrangement
            let mut scaled: Vec<_> = init.iter().map(|&(a, wr)| (a, wr * first.prob)).collect();
            // Add all invalid actions with winrate 0 at the end.
            scaled.extend(
                all_actions
                    .iter()
                    .filter(|a| !init.iter().any(|e| e.0 == **a))
                    .map(|&a| (a, Frac::ZERO)),
            );
            scaled
        };

        if initial_actions.is_empty() {
            return Err("couldn't find any winning actions".to_string());
        }

        let (best_perform, best_winrate) = if hypos.len() > 1 {
            let mut best = initial_actions[0];

            for &(action, first_winrate) in &initial_actions {
                if Instant::now() > deadline {
                    break;
                }
                let best_winrate = best.1;

                log_highlight!(
                    logger::GREEN,
                    "\ntesting action: {}",
                    action.fmt(game, None)
                );

                let mut winrate = first_winrate;
                // The probability still to be tested is everything but the first arrangement.
                let mut rem_prob = Frac::ONE - first.prob;

                for hypo in &hypos[1..] {
                    if winrate + rem_prob < best_winrate {
                        log_info!(
                            "action {} has winrate {winrate} {rem_prob}, can't add up to {best_winrate}",
                            action.fmt(game, None)
                        );
                        break;
                    }

                    if let Some(perform) = hypo.actions.iter().find(|a| a.0 == action) {
                        let (undrawn, drawn) = &hypo.arrs;
                        let arrs = if perform.0.is_clue() { undrawn } else { drawn };
                        winrate += hypo.prob
                            * self.action_winrate(&hypo.game, arrs, perform, our, deadline);
                    }
                    rem_prob -= hypo.prob;
                }

                if winrate == Frac::ONE {
                    best = (action, winrate);
                    break;
                } else if winrate > best_winrate {
                    best = (action, winrate);
                }
            }
            best
        } else {
            initial_actions.swap_remove(0)
        };

        if best_winrate == Frac::ZERO {
            Err("couldn't find any winning actions".to_string())
        } else {
            log_info!(
                "endgame winnable! {} (winrate {best_winrate})",
                best_perform.fmt(game, None)
            );
            log_info!("solved in {}ms", start.elapsed().as_millis());
            Ok((best_perform, best_winrate))
        }
    }

    /// Finds a winning sequence of actions (the first of which is the one to take) and its winrate.
    pub fn winnable(
        &mut self,
        game: &Game,
        player_turn: usize,
        remaining: &RemainingMap,
        deadline: Instant,
        depth: usize,
    ) -> WinnableResult {
        let state = &game.state;

        if Instant::now() > deadline {
            return Err(TIMEOUT);
        }

        let trivial_win = trivially_winnable(game, player_turn);
        if trivial_win.is_ok() {
            log_info!("{}trivially winnable!", indent(depth));
            return trivial_win;
        }

        if let Some(clueless_win) = self.clueless_win(game, player_turn, remaining, deadline, depth)
        {
            log_info!("{}clueless winnable!", indent(depth));

            // Replace dummy action
            return if clueless_win
                == (PerformAction::Rank {
                    target: 0,
                    value: 0,
                }) {
                Ok((
                    game.conv
                        .find_all_clues(game, player_turn)
                        .into_iter()
                        .take(1)
                        .collect(),
                    Frac::ONE,
                ))
            } else {
                Ok((vec![clueless_win], Frac::ONE))
            };
        }

        let bottom_decked = !remaining.is_empty()
            && remaining
                .iter()
                .all(|(id, _)| state.is_critical(id) && id.rank < state.max_ranks[id.suit_index]);

        if bottom_decked || unwinnable_state(state, player_turn, remaining, depth) {
            return Err(UNWINNABLE);
        }

        let performs = self.possible_actions(game, player_turn, remaining, deadline, depth, false);

        if performs.is_empty() {
            return Err(UNWINNABLE);
        }

        if state.score() + 1 == state.max_score() {
            let winning_play = performs.iter().find(|(p, _)| {
                matches!(p, PerformAction::Play { target }
                    if state.deck[*target].id().is_some_and(|id| state.is_playable(id)))
            });

            if let Some((p, _)) = winning_play {
                return Ok((vec![*p], Frac::ONE));
            }
        }

        let one_bdr_left = state.score() + 2 == state.max_score()
            && state
                .play_stacks
                .iter()
                .enumerate()
                .filter(|&(i, &stack)| stack == state.max_ranks[i])
                .count()
                == state.variant.suits.len() - 1
            && state
                .playable_set
                .first()
                .is_some_and(|id| state.is_critical(id));

        if one_bdr_left
            && let Some(bdr_id) = state.playable_set.first()
            && let Some(final_id) = bdr_id.next()
        {
            let unseen = visible_find(state, game.me(), bdr_id, VisibleOpts::default(), |_, _| {
                true
            })
            .is_empty();

            let known5 = visible_find(
                state,
                game.me(),
                final_id,
                VisibleOpts::default(),
                |_, _| true,
            )
            .into_iter()
            .any(|o| game.players[state.holder_of[o]].thoughts[o].matches(&final_id));

            let navigable = state.clue_tokens + state.pace().max(0) as usize > state.num_players
                && state.cards_left > 2;

            if unseen && known5 && navigable {
                let last = performs.last().expect("performs is not empty").0;
                return Ok((
                    vec![last],
                    Frac::new(state.cards_left as i64 - 1, state.cards_left as i64),
                ));
            }
        }

        log_highlight!(
            logger::GREEN,
            "{}actions: {}",
            indent(depth),
            performs
                .iter()
                .map(|(p, _)| p.fmt_obj(game, player_turn))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let arrs = gen_arrs(game, remaining, player_turn, false);
        self.optimize(game, &arrs, &performs, player_turn, deadline, depth)
    }

    /// Whether everyone can win if all their unknown cards are exactly the ones we need.
    fn clueless_win(
        &self,
        game: &Game,
        player_turn: usize,
        remaining: &RemainingMap,
        deadline: Instant,
        depth: usize,
    ) -> Option<PerformAction> {
        let state = &game.state;

        let mut clueless = state.clone();
        let mut modified = FastBitSet::EMPTY;

        for suit_index in 0..state.variant.suits.len() {
            for rank in state.play_stacks[suit_index] + 1..=state.max_ranks[suit_index] {
                let id = Identity::new(suit_index, rank);

                let order = state
                    .held_orders
                    .iter()
                    .find(|&o| game.common.thoughts[o].matches_infer(&id))
                    .filter(|&o| state.deck[o].id().is_none_or(|i| i == id))?;

                clueless.deck[order].identity = Some(id);
                modified.insert(order);
            }
        }

        for order in state.held_orders.difference(modified) {
            clueless.deck[order].identity = None;
        }

        clueless_winnable(&clueless, player_turn, remaining, deadline, depth)
    }

    /// The actions that could win, in the order they should be tried, with the draws that keep them winning.
    pub fn possible_actions(
        &self,
        game: &Game,
        player_turn: usize,
        remaining: &RemainingMap,
        deadline: Instant,
        depth: usize,
        infer: bool,
    ) -> Actions {
        let state = &game.state;

        let try_action = |perform: PerformAction, intended: Option<PerformAction>| {
            let intended = intended.unwrap_or(perform);

            match winnable_if(state, player_turn, intended, remaining, deadline, depth) {
                SimpleResult::Unwinnable => None,
                SimpleResult::WinnableWithDraws(draws) => Some((perform, draws)),
                SimpleResult::AlwaysWinnable => Some((perform, Vec::new())),
            }
        };

        let urgent = state.hands[player_turn]
            .iter()
            .copied()
            .find(|&o| game.meta[o].urgent);

        if let Some(urgent) = urgent {
            let perform = match game.meta[urgent].status {
                CardStatus::DragToPlay => PerformAction::Play { target: urgent },
                CardStatus::CalledToPlay => PerformAction::try_play(game, urgent),
                _ => PerformAction::try_discard(game, urgent),
            };

            if let Some(action) = try_action(perform, None) {
                return vec![action];
            }
        }

        let playables = {
            let plays = if infer || game.good_touch() || state.endgame_turns.is_some() {
                game.players[player_turn].thinks_playables(game, player_turn, true, true)
            } else {
                game.players[player_turn].obvious_playables(game, player_turn)
            };

            let slot1 = state.hands[player_turn][0];
            let poss = game.players[player_turn].thoughts[slot1].possibilities();

            // Allow bombing slot 1 when no plays and a playable card could be in slot 1
            if state.endgame_turns.is_some()
                && plays.is_empty()
                && !poss.intersect(state.playable_set).is_empty()
            {
                vec![slot1]
            } else {
                plays
            }
        };

        let mut play_actions = Vec::new();
        for &order in &playables {
            if Instant::now() > deadline {
                return Vec::new();
            }

            if state.deck[order].id().is_none() {
                continue;
            }

            play_actions.extend(try_action(
                PerformAction::try_play(game, order),
                Some(PerformAction::Play { target: order }),
            ));
        }

        let default_clue = PerformAction::Rank {
            target: 0,
            value: 0,
        };
        let too_many_clues = state.num_players > 2
            && state
                .action_list
                .iter()
                .flatten()
                .rev()
                .take_while(|a| !a.requires_draw())
                .filter(|a| matches!(a, Action::Clue(_)))
                .count()
                > state.num_players + 1;

        let clue_winnable = state.can_clue()
            && !too_many_clues
            && match winnable_if(state, player_turn, default_clue, remaining, deadline, depth) {
                SimpleResult::Unwinnable => false,
                SimpleResult::AlwaysWinnable => true,
                SimpleResult::WinnableWithDraws(_) => {
                    panic!("Shouldn't return WinnableWithDraws from giving a clue!")
                }
            };

        let clue_actions: Actions = if !clue_winnable {
            Vec::new()
        } else {
            // If everyone knows exactly where all the remaining useful cards are, clues are only useful
            // for stalling, so we only need to consider 1 clue
            let fully_known = (remaining.is_empty()
                || (remaining.iter().count() == 1
                    && remaining
                        .first()
                        .is_some_and(|(id, _)| state.is_basic_trash(id))))
                && state.held_orders.iter().all(|o| {
                    state.deck[o].id().is_none_or(|id| {
                        state.is_basic_trash(id) || game.common.thoughts[o].matches_infer(&id)
                    })
                });

            let all_clues = game.conv.find_all_clues(game, player_turn);
            let take = if fully_known { 1 } else { all_clues.len() };
            all_clues
                .into_iter()
                .take(take)
                .map(|c| (c, Vec::new()))
                .collect()
        };

        if Instant::now() > deadline {
            return Vec::new();
        }

        let hand = &state.hands[player_turn];
        let ignore_dc = state.pace() == 0
            || state.clue_tokens == 8
            || (state.variant.name == "No Variant"
                && playables.iter().any(|&p| {
                    game.players[player_turn].thoughts[p]
                        .id_with(true, false)
                        .is_some_and(|id| {
                            // Always play a 5 over dc
                            id.rank == 5
                                || (hand.iter().any(|&o| {
                                    o != p
                                        && state.deck[o].clued
                                        && game.common.thoughts[o]
                                            .possible
                                            .difference(state.critical_set)
                                            .is_empty()
                                }) && (state.is_critical(id)
                                    // Always play a duped card in hand over dc
                                    || hand.iter().any(|&o| {
                                        o != p
                                            && game.players[player_turn].thoughts[o]
                                                .matches_infer(&id)
                                    })))
                        })
                }))
            || {
                let holding_crit = hand
                    .iter()
                    .any(|&o| state.deck[o].id().is_some_and(|id| state.is_critical(id)));

                // Density of crits is too high, can't discard
                holding_crit
                    && remaining.iter().all(|(id, _)| state.is_critical(id))
                    && state.clue_tokens < state.num_players - 1
            };

        let dc_actions: Actions = if ignore_dc {
            Vec::new()
        } else {
            game.conv
                .find_all_discards(game, player_turn)
                .into_iter()
                .filter_map(|p| try_action(p, None))
                .collect()
        };

        // If no playables are visible, try discarding before cluing
        let prefer_dc = !ignore_dc
            && (game.conv.prefer_endgame_discard(game, player_turn)
                || state.hands.iter().enumerate().all(|(i, hand)| {
                    i == player_turn
                        || hand
                            .iter()
                            .all(|&o| !state.deck[o].id().is_some_and(|id| state.is_playable(id)))
                }));

        let (first, second, third) =
            if depth == 0 && clue_winnable && game.conv.prefer_endgame_clue(game) {
                (clue_actions, play_actions, dc_actions)
            } else if prefer_dc {
                (play_actions, dc_actions, clue_actions)
            } else {
                (play_actions, clue_actions, dc_actions)
            };

        first.into_iter().chain(second).chain(third).collect()
    }

    /// The winrate of performing the action, summed over the possible draws.
    pub fn action_winrate(
        &mut self,
        game: &Game,
        arrs: &[GameArr],
        action: &(PerformAction, Vec<Identity>),
        player_turn: usize,
        deadline: Instant,
    ) -> Frac {
        if Instant::now() > deadline {
            return Frac::ZERO;
        }

        let (perform, winnable_draws) = action;
        let next_player_index = game.state.next_player_index(player_turn);
        let mut total = Frac::ZERO;

        for GameArr {
            prob,
            remaining,
            drew,
        } in arrs
        {
            if drew.is_some_and(|d| !winnable_draws.contains(&d)) {
                continue;
            }

            let new_game = game.simulate_action(
                &perform.to_action(&game.state, player_turn, None, false),
                *drew,
                false,
            );

            if new_game.state.max_score() < game.state.max_score() {
                continue;
            }

            log_info!(
                "{} cards left {} endgame turns {:?}",
                perform.fmt_obj(game, player_turn),
                new_game.state.cards_left,
                new_game.state.endgame_turns
            );

            match self.winnable(&new_game, next_player_index, remaining, deadline, 1) {
                Err(msg) => {
                    log_highlight!(
                        logger::YELLOW,
                        "}} {} unwinnable ({msg})",
                        perform.fmt_obj(game, player_turn)
                    );
                }
                Ok((_, wr)) => total += *prob * wr,
            }
        }

        total
    }

    /// The winrate of every action, best first. If `shortcut` is set, stops at the first guaranteed win.
    pub fn optimize_full(
        &mut self,
        game: &Game,
        arrs: &(Vec<GameArr>, Vec<GameArr>),
        actions: &Actions,
        player_turn: usize,
        deadline: Instant,
        shortcut: bool,
    ) -> Vec<(PerformAction, Frac)> {
        let (undrawn, drawn) = arrs;
        let mut results = Vec::new();

        for action in actions {
            let arrs = if action.0.is_clue() { undrawn } else { drawn };
            let winrate = self.action_winrate(game, arrs, action, player_turn, deadline);

            if shortcut && winrate == Frac::ONE {
                return vec![(action.0, winrate)];
            }
            results.push((action.0, winrate));
        }

        results.sort_by_key(|a| std::cmp::Reverse(a.1));
        results
    }

    /// Finds the best actions (those tied for the highest winrate) by trying every action.
    pub fn optimize(
        &mut self,
        game: &Game,
        arrs: &(Vec<GameArr>, Vec<GameArr>),
        actions: &Actions,
        player_turn: usize,
        deadline: Instant,
        depth: usize,
    ) -> WinnableResult {
        let (undrawn, drawn) = arrs;
        let next_player_index = game.state.next_player_index(player_turn);

        // Try the actions that have worked best at this depth first.
        let mut sorted_actions: Vec<&(PerformAction, Vec<Identity>)> = actions.iter().collect();
        if let Some(sr) = self.success_rate.get(&depth) {
            sorted_actions.sort_by_key(|a| -sr.get(&a.0).map_or(Frac::ONE, |&(frac, _)| frac));
        }

        let mut best_actions: Vec<PerformAction> = Vec::new();
        let mut best_winrate = Frac::ZERO;

        for (perform, winnable_draws) in sorted_actions {
            if Instant::now() > deadline {
                return Ok((best_actions, best_winrate));
            }

            let mut winrate = Frac::ZERO;
            let mut rem_prob = Frac::ONE;

            for GameArr {
                prob,
                remaining,
                drew,
            } in if perform.is_clue() { undrawn } else { drawn }
            {
                if winrate + rem_prob < best_winrate {
                    break;
                }

                let new_prob = rem_prob - *prob;
                let new_game = (!drew.is_some_and(|d| !winnable_draws.contains(&d))).then(|| {
                    game.simulate_action(
                        &perform.to_action(&game.state, player_turn, None, false),
                        *drew,
                        false,
                    )
                });

                let unwinnable = match &new_game {
                    None => true,
                    Some(g) => {
                        g.state.max_score() < game.state.max_score()
                            || g.state.pace() < 0
                            || (!remaining.is_empty()
                                && remaining.iter().all(|(id, _)| {
                                    g.state.is_critical(id)
                                        && id.rank < g.state.max_ranks[id.suit_index]
                                }))
                    }
                };

                if unwinnable {
                    rem_prob = new_prob;
                    continue;
                }
                let new_game = new_game.expect("checked above");

                log_info!(
                    "{}{} cards left {} endgame turns {:?}",
                    indent(depth),
                    perform.fmt_obj(game, player_turn),
                    new_game.state.cards_left,
                    new_game.state.endgame_turns
                );

                let colour = if depth == 0 {
                    logger::YELLOW
                } else {
                    logger::WHITE
                };
                match self.winnable(&new_game, next_player_index, remaining, deadline, depth + 1) {
                    Err(msg) => {
                        log_highlight!(
                            colour,
                            "{}}} {} unwinnable ({msg})",
                            indent(depth),
                            perform.fmt_obj(game, player_turn)
                        );
                    }
                    Ok((performs, wr)) => {
                        let new_winrate = winrate + *prob * wr;
                        assert!(
                            new_winrate <= Frac::ONE,
                            "Winrate exceeds 100% {prob} {new_winrate}"
                        );

                        log_highlight!(
                            colour,
                            "{}}} {} prob {prob} winrate {wr}",
                            indent(depth),
                            performs
                                .iter()
                                .map(|p| p.fmt_obj(game, next_player_index))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        winrate = new_winrate;
                    }
                }
                // This arrangement is settled, whatever the result.
                rem_prob = new_prob;
            }

            let entry = self.success_rate.entry(depth).or_default();
            match entry.get_mut(perform) {
                Some((frac, times)) => {
                    *frac = (*frac * *times + winrate) / (*times + 1);
                    *times += 1;
                }
                None => {
                    entry.insert(*perform, (winrate, 1));
                }
            }

            let state = &game.state;
            let bdrs = state
                .all_ids
                .difference(state.trash_set)
                .iter()
                .filter(|&id| state.is_useful(id) && state.is_critical(id) && id.rank != 5)
                .count();
            let cards_left = state.cards_left as i64;

            if winrate == Frac::ONE
                || (bdrs == 1 && cards_left > 1 && winrate == Frac::new(cards_left - 1, cards_left))
            {
                return Ok((vec![*perform], winrate));
            }

            if winrate > best_winrate {
                best_actions = vec![*perform];
                best_winrate = winrate;
            } else if winrate > Frac::ZERO && winrate == best_winrate {
                best_actions.push(*perform);
            }
        }

        if best_actions.is_empty() {
            Err("no action wins")
        } else {
            Ok((best_actions, best_winrate))
        }
    }
}
