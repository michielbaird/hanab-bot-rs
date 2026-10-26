//! Port of `test/util.scala` and `test/exAsserts.scala`.
#![allow(dead_code)]

use std::sync::{Arc, LazyLock};

use regex::Regex;
use scala_bot::basics::action::{
    Action, ClueAction, DiscardAction, DrawAction, PerformAction, PlayAction, TurnAction,
};
use scala_bot::basics::clue::{BaseClue, CardClue, ClueKind};
use scala_bot::basics::game::Base;
use scala_bot::basics::state::TableOptions;
use scala_bot::basics::*;
use scala_bot::collections::FastBitSet;
use scala_bot::logger::{self, LogLevel};
use scala_bot::util::{VisibleOpts, visible_find};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colour {
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl Colour {
    pub fn ordinal(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Player {
    Alice,
    Bob,
    Cathy,
    Donald,
    Emily,
}

impl Player {
    pub fn ordinal(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestVariant {
    NoVariant,
    NoVar6,
    Rainbow5,
    Black5,
    White5,
    Pink5,
    Brown5,
    Prism5,
    Muddy5,
    Cocoa5,
    Omni5,
    Null5,
    Orange5,
    DBrown5,
    DOrange5,
    PinkLPink6,
}

impl TestVariant {
    pub fn variant(self) -> Arc<Variant> {
        use TestVariant::*;
        let (id, name, suits, shorts): (u32, &str, &[&str], &[char]) = match self {
            NoVariant => (
                0,
                "No Variant",
                &["Red", "Yellow", "Green", "Blue", "Purple"],
                &['r', 'y', 'g', 'b', 'p'],
            ),
            NoVar6 => (
                1,
                "6 Suits",
                &["Red", "Yellow", "Green", "Blue", "Purple", "Teal"],
                &['r', 'y', 'g', 'b', 'p', 't'],
            ),
            Rainbow5 => (
                16,
                "Rainbow (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Rainbow"],
                &['r', 'y', 'g', 'b', 'm'],
            ),
            Black5 => (
                21,
                "Black (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Black"],
                &['r', 'y', 'g', 'b', 'k'],
            ),
            White5 => (
                22,
                "White (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "White"],
                &['r', 'y', 'g', 'b', 'w'],
            ),
            Pink5 => (
                107,
                "Pink (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Pink"],
                &['r', 'y', 'g', 'b', 'i'],
            ),
            Brown5 => (
                70,
                "Brown (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Brown"],
                &['r', 'y', 'g', 'b', 'n'],
            ),
            Prism5 => (
                1465,
                "Prism (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Prism"],
                &['r', 'y', 'g', 'b', 'i'],
            ),
            Muddy5 => (
                161,
                "Muddy Rainbow (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Muddy Rainbow"],
                &['r', 'y', 'g', 'b', 'm'],
            ),
            Cocoa5 => (
                291,
                "Cocoa Rainbow (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Cocoa Rainbow"],
                &['r', 'y', 'g', 'b', 'm'],
            ),
            Omni5 => (
                177,
                "Omni (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Omni"],
                &['r', 'y', 'g', 'b', 'o'],
            ),
            Null5 => (
                181,
                "Null (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Null"],
                &['r', 'y', 'g', 'b', 'u'],
            ),
            Orange5 => (
                2131,
                "Orange (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Orange"],
                &['r', 'y', 'g', 'b', 'o'],
            ),
            DBrown5 => (
                90,
                "Dark Brown (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Dark Brown"],
                &['r', 'y', 'g', 'b', 'n'],
            ),
            DOrange5 => (
                2135,
                "Dark Orange (5 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Dark Orange"],
                &['r', 'y', 'g', 'b', 'o'],
            ),
            PinkLPink6 => (
                1296,
                "Pink & Light Pink (6 Suits)",
                &["Red", "Yellow", "Green", "Blue", "Pink", "Light Pink"],
                &['r', 'y', 'g', 'b', 'i', 'l'],
            ),
        };
        Arc::new(Variant::with_shorts(id, name, suits, shorts))
    }
}

pub const NAMES: [&str; 5] = ["Alice", "Bob", "Cathy", "Donald", "Emily"];

/// A convention that interprets nothing: clues, plays and discards only update empathy.
///
/// Used by tests of the convention-independent model. Tests that port a Scala test run against
/// Reactor or H-Group use this until those conventions are ported.
#[derive(Debug)]
pub struct Neutral {
    pub good_touch: bool,
}

impl Convention for Neutral {
    fn name(&self) -> String {
        "Neutral".to_string()
    }
    fn good_touch(&self) -> bool {
        self.good_touch
    }
    fn interpret_clue(&self, _prev: &Game, game: &mut Game, _action: &ClueAction) {
        game.elim(None, false);
        game.with_move(ClueInterp::Play.into(), true);
    }
    fn interpret_discard(&self, _prev: &Game, game: &mut Game, _action: &DiscardAction) {
        game.elim(None, false);
        game.with_move(DiscardInterp::None.into(), true);
    }
    fn interpret_play(&self, _prev: &Game, game: &mut Game, _action: &PlayAction) {
        game.elim(None, false);
        game.with_move(PlayInterp::None.into(), true);
    }
    fn take_action(&self, _game: &Game) -> PerformAction {
        unimplemented!("the neutral convention never acts")
    }
    fn update_turn(&self, _game: &mut Game, _action: &TurnAction) {}
    /// Every clue that touches at least one card, to each other player in turn order.
    fn find_all_clues(&self, game: &Game, giver: usize) -> Vec<PerformAction> {
        let state = &game.state;
        (1..state.num_players)
            .map(|offset| (giver + offset) % state.num_players)
            .flat_map(|target| state.all_valid_clues(target))
            .map(|clue| PerformAction::from_clue(&clue))
            .collect()
    }
    /// Discards the oldest card.
    fn find_all_discards(&self, game: &Game, player_index: usize) -> Vec<PerformAction> {
        vec![PerformAction::Discard {
            target: *game.state.hands[player_index].last().unwrap(),
        }]
    }
    fn eval_action(&self, _game: &Game, _action: &Action) -> f64 {
        0.0
    }
}

pub fn neutral() -> Arc<dyn Convention> {
    Arc::new(Neutral { good_touch: true })
}

pub struct Setup<'a> {
    pub play_stacks: Option<Vec<usize>>,
    pub discarded: &'a [&'a str],
    pub strikes: usize,
    pub clue_tokens: usize,
    pub starting: Player,
    pub variant: TestVariant,
    pub init: Box<dyn Fn(&mut Game) + 'a>,
}

impl Default for Setup<'_> {
    fn default() -> Self {
        Self {
            play_stacks: None,
            discarded: &[],
            strikes: 0,
            clue_tokens: 8,
            starting: Player::Alice,
            variant: TestVariant::NoVariant,
            init: Box::new(|_| {}),
        }
    }
}

fn certain_for_stacks(player: &mut scala_bot::basics::Player, stacks: &[usize]) {
    let ids = stacks.iter().enumerate().flat_map(|(suit_index, &stack)| {
        (1..=stack).map(move |rank| Identity::new(suit_index, rank))
    });
    for (i, id) in ids.enumerate() {
        player.certain_map[id.to_ord()].update(63 - i, None);
    }
}

pub fn setup(conv: Arc<dyn Convention>, hands: &[&[&str]], opts: Setup) -> Game {
    logger::set_level(LogLevel::Off);

    let player_names: Vec<String> = NAMES[..hands.len()].iter().map(|s| s.to_string()).collect();
    let variant = opts.variant.variant();
    let state = State::new(
        player_names.clone(),
        0,
        variant.clone(),
        TableOptions::new(hands.len(), &variant.name),
    );

    let mut game = Game::new(0, state, false, conv);
    game.catchup = true;

    if let Some(stacks) = &opts.play_stacks {
        assert_eq!(
            stacks.len(),
            game.state.variant.suits.len(),
            "Invalid play stacks length"
        );

        game.state.play_stacks = stacks.clone();
        for (suit_index, &stack) in stacks.iter().enumerate() {
            for rank in 1..=stack {
                game.state.base_count[Identity::new(suit_index, rank).to_ord()] += 1;
            }
        }
        for p in game.players.iter_mut().chain([&mut game.common]) {
            p.hypo_stacks = stacks.clone();
            certain_for_stacks(p, stacks);
        }
    }

    assert!(
        !hands.iter().any(|h| h.len() > HAND_SIZE[hands.len()]),
        "Hand size should be {} for a {}-player game!",
        HAND_SIZE[hands.len()],
        hands.len()
    );

    // Draw all the hands
    let mut order_counter = 0;
    for (player_index, hand) in hands.iter().enumerate() {
        for short in hand.iter().rev() {
            let id = (*short != "xx").then(|| game.state.expand_short(short).unwrap());
            game.handle_action(&Action::Draw(DrawAction {
                player_index,
                order: order_counter,
                id,
            }))
            .unwrap();
            order_counter += 1;
        }
    }

    let score = game.state.score();
    for (i, short) in opts.discarded.iter().enumerate() {
        let id = game.state.expand_short(short).unwrap();
        let order = 63 - score - i;
        game.state.add_discard(id, order);

        for p in game.players.iter_mut().chain([&mut game.common]) {
            p.certain_map[id.to_ord()].update(order, None);
        }
    }

    {
        let s = &mut game.state;
        let trash_set = s.all_ids.filter(|id| {
            id.rank <= s.play_stacks[id.suit_index] || id.rank > s.max_ranks[id.suit_index]
        });
        s.cards_left = s.cards_left - s.score() - opts.discarded.len();
        s.current_player_index = opts.starting.ordinal();
        s.clue_tokens = opts.clue_tokens;
        s.strikes = opts.strikes;
        s.playable_set = s.all_ids.difference(trash_set).filter(|id| {
            id.rank <= s.max_ranks[id.suit_index] && id.rank == s.play_stacks[id.suit_index] + 1
        });
        s.critical_set = s.all_ids.difference(trash_set).filter(|id| {
            s.discard_stacks[id.suit_index][id.rank - 1].len() == s.card_count[id.to_ord()] - 1
        });
        s.trash_set = trash_set;
    }

    for id in game.state.variant.all_ids() {
        let count = game.state.base_count[id.to_ord()]
            + visible_find(
                &game.state,
                game.me(),
                id,
                VisibleOpts::default(),
                |_, _| true,
            )
            .len();

        assert!(
            count <= game.state.card_count[id.to_ord()],
            "Found {count} copies of {}!",
            game.state.log_id(id)
        );
    }

    (opts.init)(&mut game);
    game.elim(None, false);

    game.base = Arc::new(Base {
        state: game.state.clone(),
        meta: game.meta.clone(),
        players: game.players.clone(),
        common: game.common.clone(),
    });
    game
}

pub fn take_turn(game: &mut Game, raw_action: &str) {
    take_turn_draw(game, raw_action, "");
}

pub fn take_turn_draw(game: &mut Game, raw_action: &str, draw_s: &str) {
    let state = game.state.clone();
    let action = parse_action(&state, raw_action);
    let draw = (!draw_s.is_empty()).then(|| state.expand_short(draw_s).unwrap());
    let acting = action.player_index().unwrap();

    assert_eq!(
        acting,
        state.current_player_index,
        "Expected '{}'s turn for action ({})!",
        state.names[state.current_player_index],
        action.fmt(&state)
    );

    game.with_catchup(|g| {
        g.handle_action(&action).unwrap();

        match &action {
            Action::Play(_) | Action::Discard(_) => match draw {
                Some(_) if state.cards_left == 0 => panic!("Cannot draw at 0 cards left"),
                None if acting != state.our_player_index => {
                    panic!(
                        "Missing draw for {}'s action {action:?}",
                        state.names[acting]
                    )
                }
                Some(id) => {
                    let count = state.base_count[id.to_ord()]
                        + visible_find(&state, g.me(), id, VisibleOpts::default(), |_, _| true)
                            .len();
                    assert!(
                        count < state.card_count[id.to_ord()],
                        "Found {} copies of {}",
                        count + 1,
                        state.log_id(id)
                    );
                    g.handle_action(&Action::Draw(DrawAction {
                        player_index: state.current_player_index,
                        order: state.next_card_order,
                        id: Some(id),
                    }))
                    .unwrap();
                }
                None => {
                    g.handle_action(&Action::Draw(DrawAction {
                        player_index: state.current_player_index,
                        order: state.next_card_order,
                        id: None,
                    }))
                    .unwrap();
                }
            },
            _ => assert!(draw.is_none(), "Unexpected draw for action {raw_action}"),
        }

        g.handle_action(&Action::Turn(TurnAction {
            num: state.turn_count,
            current_player_index: Some(state.next_player_index(acting)),
        }))
        .unwrap();
    });
}

pub fn str_to_clue(state: &State, s: &str) -> BaseClue {
    if "12345".contains(s) {
        return BaseClue::new(ClueKind::Rank, s.parse().unwrap());
    }

    let colour = state
        .variant
        .suits
        .iter()
        .position(|suit| Variant::strip_name(&suit.name).to_lowercase() == s.to_lowercase())
        .unwrap_or_else(|| panic!("Colour {s} not found in suits"));
    BaseClue::new(ClueKind::Colour, colour)
}

static CLUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\w+) clues (\d|\w+) to (\w+)(?: \(slots? (\d(?:,\d)*)\))?$").unwrap()
});
static PLAY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\w+) plays (\w\d)(?: \(slot (\d)\))?$").unwrap());
static DISCARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\w+) (discards|bombs) (\w\d)(?: \(slot (\d)\))?$").unwrap());

pub fn parse_action(state: &State, action: &str) -> Action {
    let parse_player = |name: &str| {
        state
            .names
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| panic!("Player {name} not found in {:?}", state.names))
    };

    // The order to play/discard: found by identity (and slot, if ambiguous) for others, by slot for us.
    let find_order = |player_index: usize,
                      id: Identity,
                      short: &str,
                      slot: Option<&str>,
                      what: &str| {
        if player_index != state.our_player_index {
            let matching: Vec<usize> = state.hands[player_index]
                .iter()
                .copied()
                .filter(|&o| state.deck[o].identity == Some(id))
                .collect();

            match matching.as_slice() {
                [] => panic!(
                    "Unable to find {short} to {what} in {}'s hand",
                    state.names[player_index]
                ),
                [order] => {
                    if let Some(slot) = slot {
                        let slot: usize = slot.parse().unwrap();
                        assert_eq!(
                            state.hands[player_index][slot - 1],
                            *order,
                            "Identity {short} not in slot {slot}"
                        );
                    }
                    *order
                }
                _ => {
                    let slot = slot.unwrap_or_else(|| {
                        panic!("Not enough arguments provided (ambiguous identity) in {action}, needs '(slot x)'")
                    });
                    let order = state.hands[player_index][slot.parse::<usize>().unwrap() - 1];
                    assert_eq!(
                        state.deck[order].identity,
                        Some(id),
                        "Identity {short} not in slot {slot}"
                    );
                    order
                }
            }
        } else {
            let slot = slot.unwrap_or_else(|| {
                panic!(
                    "Not enough arguments provided ({what} from us) in {action}, needs '(slot x)'"
                )
            });
            state.hands[player_index][slot.parse::<usize>().unwrap() - 1]
        }
    };

    if let Some(c) = CLUE_RE.captures(action) {
        assert!(state.can_clue(), "Tried to clue with 0 clue tokens");
        assert_ne!(&c[1], &c[3], "{} cannot clue themselves!", &c[1]);

        let (giver, target) = (parse_player(&c[1]), parse_player(&c[3]));
        let clue = str_to_clue(state, &c[2]);

        if target != state.our_player_index {
            let list = state.clue_touched(&state.hands[target], &clue);
            assert!(
                !list.is_empty(),
                "No cards touched by clue ({} to {})!",
                &c[2],
                &c[3]
            );
            Action::Clue(ClueAction {
                giver,
                target,
                list,
                clue,
            })
        } else {
            let slots = c.get(4).unwrap_or_else(|| {
                panic!("Not enough arguments provided (clue to us) in {action}, needs '(slot x)'")
            });
            let list = slots
                .as_str()
                .split(',')
                .map(|s| state.our_hand()[s.parse::<usize>().unwrap() - 1])
                .collect();
            Action::Clue(ClueAction {
                giver,
                target,
                list,
                clue,
            })
        }
    } else if let Some(c) = PLAY_RE.captures(action) {
        let player_index = parse_player(&c[1]);
        let id = state.expand_short(&c[2]).unwrap();
        let order = find_order(
            player_index,
            id,
            &c[2],
            c.get(3).map(|m| m.as_str()),
            "play",
        );
        Action::Play(PlayAction {
            player_index,
            order,
            id: Some(id),
        })
    } else if let Some(c) = DISCARD_RE.captures(action) {
        assert!(
            state.clue_tokens != 8 || &c[2] == "bombs",
            "Tried to discard with 8 clue tokens"
        );

        let player_index = parse_player(&c[1]);
        let id = state.expand_short(&c[3]).unwrap();
        let order = find_order(
            player_index,
            id,
            &c[3],
            c.get(4).map(|m| m.as_str()),
            "discard",
        );
        Action::Discard(DiscardAction {
            player_index,
            order,
            id: Some(id),
            failed: &c[2] == "bombs",
        })
    } else {
        panic!("Invalid action: {action}")
    }
}

#[derive(Clone, Copy)]
pub struct TestClue {
    pub kind: ClueKind,
    pub value: usize,
    pub giver: Player,
}

impl TestClue {
    pub fn to_base(self) -> BaseClue {
        BaseClue::new(self.kind, self.value)
    }
}

/// Pre-clues the card in the given slot with the given clues (by name, like `red` or `4`).
pub fn pre_clue(game: &mut Game, player: Player, slot: usize, clues: &[&str]) {
    let test_clues: Vec<TestClue> = clues
        .iter()
        .map(|c| {
            let base = str_to_clue(&game.state, c);
            TestClue {
                kind: base.kind,
                value: base.value,
                giver: if player == Player::Alice {
                    Player::Bob
                } else {
                    Player::Alice
                },
            }
        })
        .collect();

    pre_clue_with(game, player, slot, &test_clues);
}

pub fn pre_clue_with(game: &mut Game, player: Player, slot: usize, clues: &[TestClue]) {
    let order = game.state.hands[player.ordinal()][slot - 1];

    if let Some(id) = game.state.deck[order].identity
        && let Some(bad) = clues
            .iter()
            .find(|c| !game.state.variant.id_touched(id, &c.to_base()))
    {
        panic!(
            "Clue {} doesn't touch order {order}!",
            scala_bot::basics::clue::ClueLike::fmt(&bad.to_base(), &game.state, player.ordinal())
        );
    }

    let possibilities: IdentitySet = game
        .state
        .variant
        .all_ids()
        .filter(|&i| {
            clues
                .iter()
                .all(|c| game.state.variant.id_touched(i, &c.to_base()))
        })
        .collect();

    game.common.update_thought(order, |t| {
        t.inferred = possibilities.intersect(t.possible);
        t.possible = possibilities.intersect(t.possible);
    });

    let card = &mut game.state.deck[order];
    card.clued = true;
    card.clues = clues
        .iter()
        .map(|c| CardClue {
            kind: c.kind,
            value: c.value,
            giver: c.giver.ordinal(),
            turn: 0,
        })
        .collect();
}

/// Pre-clues the slot with both colour and rank (only works for simple variants).
pub fn fully_known(game: &mut Game, player: Player, slot: usize, short: &str) {
    let state = &game.state;
    let card = &state.deck[state.hands[player.ordinal()][slot - 1]];
    let id = state.expand_short(short).unwrap();

    if let Some(found) = card.identity {
        assert_eq!(
            found,
            id,
            "{}'s card at slot {slot} is not {}! found {}",
            state.names[player.ordinal()],
            state.log_id(id),
            state.log_id(found)
        );
    }

    let giver = if player == Player::Alice {
        Player::Bob
    } else {
        Player::Alice
    };
    let colour_value =
        if scala_bot::basics::variant::PRISM.matches(&state.variant.suits[id.suit_index].name) {
            (id.rank - 1) % state.variant.colourable_suits.len()
        } else {
            id.suit_index
        };
    let clues = [
        TestClue {
            kind: ClueKind::Rank,
            value: id.rank,
            giver,
        },
        TestClue {
            kind: ClueKind::Colour,
            value: colour_value,
            giver,
        },
    ];

    pre_clue_with(game, player, slot, &clues);
}

fn thought_of(
    game: &Game,
    player_index: Option<Player>,
    target: Player,
    slot: usize,
) -> (usize, &Thought, &scala_bot::basics::Player) {
    let order = *game.state.hands[target.ordinal()]
        .get(slot - 1)
        .unwrap_or_else(|| panic!("Slot {slot} doesn't exist"));
    let player = player_index.map_or(&game.common, |p| &game.players[p.ordinal()]);
    (order, &player.thoughts[order], player)
}

fn id_set(game: &Game, shorts: &[&str]) -> IdentitySet {
    shorts
        .iter()
        .map(|s| game.state.expand_short(s).unwrap())
        .collect()
}

pub fn has_infs(
    game: &Game,
    player_index: Option<Player>,
    target: Player,
    slot: usize,
    infs: &[&str],
) {
    let (order, thought, player) = thought_of(game, player_index, target, slot);
    assert_eq!(
        thought.inferred,
        id_set(game, infs),
        "Differing inferences (order {order}). Expected {}, got {}",
        infs.join(","),
        player.str_infs(&game.state, order)
    );
}

pub fn has_poss(
    game: &Game,
    player_index: Option<Player>,
    target: Player,
    slot: usize,
    poss: &[&str],
) {
    let (order, thought, player) = thought_of(game, player_index, target, slot);
    assert_eq!(
        thought.possible,
        id_set(game, poss),
        "Differing possibilities (order {order}). Expected {}, got {}",
        poss.join(","),
        player.str_poss(&game.state, order)
    );
}

pub fn has_status(game: &Game, target: Player, slot: usize, status: CardStatus) {
    let order = *game.state.hands[target.ordinal()]
        .get(slot - 1)
        .unwrap_or_else(|| panic!("Slot {slot} doesn't exist"));
    assert_eq!(
        game.meta[order].status, status,
        "Differing status (order {order}). Expected {status}, got {}",
        game.meta[order].status
    );
}

#[allow(unused)]
fn _keep(_: FastBitSet) {}

pub fn reactor() -> Arc<dyn Convention> {
    Arc::new(scala_bot::reactor::Reactor::new())
}
