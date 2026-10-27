//! Exported games: the players, deck and every action taken.

use std::sync::Arc;

use serde_json::Value;

use super::args::parse_args;
use crate::basics::state::TableOptions;
use crate::basics::{Game, Identity, PerformAction, State, Variant};
use crate::settings::ConventionKind;

use crate::basics::HAND_SIZE;
use crate::basics::action::{Action, DrawAction, GameOverAction, TurnAction};

#[derive(Clone, Debug)]
pub struct GameData {
    pub players: Vec<String>,
    pub deck: Vec<Identity>,
    pub actions: Vec<PerformAction>,
    pub options: TableOptions,
    /// The replay id, if the export has one.
    pub id: Option<u64>,
    /// The seed of the game, if the export has one.
    pub seed: Option<String>,
    /// The notes each player wrote, by player and then card order (hanab.live exports only).
    pub notes: Vec<Vec<String>>,
}

impl GameData {
    /// Downloads an exported game from the server (blocking).
    pub fn fetch_id(hostname: &str, id: &str) -> Result<Self, String> {
        let url = format!("https://{hostname}/export/{id}");
        let text = reqwest::blocking::get(&url)
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("{url}: {e}"))?;
        Self::parse(&serde_json::from_str(&text).map_err(|e| e.to_string())?)
    }

    pub fn from_file(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Self::parse(&serde_json::from_str(&text).map_err(|e| e.to_string())?)
    }

    pub fn parse(data: &Value) -> Result<Self, String> {
        let players: Vec<String> = data["players"]
            .as_array()
            .ok_or("missing players")?
            .iter()
            .map(|p| p.as_str().map(String::from).ok_or("player is not a string"))
            .collect::<Result<_, _>>()?;

        let deck = data["deck"]
            .as_array()
            .ok_or("missing deck")?
            .iter()
            .map(|id| {
                Ok(Identity::new(
                    id["suitIndex"].as_u64().ok_or("card without a suitIndex")? as usize,
                    id["rank"].as_u64().ok_or("card without a rank")? as usize,
                ))
            })
            .collect::<Result<_, String>>()?;

        let actions = data["actions"]
            .as_array()
            .ok_or("missing actions")?
            .iter()
            .map(PerformAction::from_json)
            .collect::<Result<_, _>>()?;

        let opts = &data["options"];
        let options = TableOptions {
            num_players: players.len(),
            variant_name: opts["variant"].as_str().unwrap_or("No Variant").to_string(),
            deck_plays: opts["deckPlays"].as_bool().unwrap_or(false),
            empty_clues: opts["emptyClues"].as_bool().unwrap_or(false),
            ..TableOptions::default()
        };

        Ok(Self {
            players,
            deck,
            actions,
            options,
            id: data["id"].as_u64(),
            seed: data["seed"].as_str().map(String::from),
            notes: data["notes"]
                .as_array()
                .map(|players| {
                    players
                        .iter()
                        .map(|notes| {
                            notes
                                .as_array()
                                .map(|n| {
                                    n.iter()
                                        .map(|note| note.as_str().unwrap_or_default().to_string())
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// Loads variants from files given as `variants=<path>` and `colours=<path>` (the documents behind
/// `variant::VARIANTS_URL` and `COLOURS_URL`), falling back to the built-in "No Variant" and "6 Suits".
pub fn load_variant(
    name: &str,
    args: &std::collections::HashMap<String, String>,
) -> Result<Arc<Variant>, String> {
    if let (Some(variants), Some(colours)) = (args.get("variants"), args.get("colours")) {
        let read = |p: &String| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
        Variant::init(&read(variants)?, &read(colours)?)?;
        return Variant::get(name);
    }

    match name {
        "No Variant" => Ok(Arc::new(Variant::with_shorts(
            0,
            "No Variant",
            &["Red", "Yellow", "Green", "Blue", "Purple"],
            &['r', 'y', 'g', 'b', 'p'],
        ))),
        "6 Suits" => Ok(Arc::new(Variant::with_shorts(
            1,
            "6 Suits",
            &["Red", "Yellow", "Green", "Blue", "Purple", "Teal"],
            &['r', 'y', 'g', 'b', 'p', 't'],
        ))),
        other => Err(format!(
            "Variant '{other}' needs the variant definitions: pass variants=<variants.json> colours=<colors.json>."
        )),
    }
}

/// Replays the exported game into a fresh game for the player at `index`.
///
/// * `hide_own`: whether to hide the identities of that player's cards (as a live game would).
pub fn build_game(
    convention: ConventionKind,
    data: &GameData,
    variant: Arc<Variant>,
    index: usize,
    hide_own: bool,
) -> Result<Game, String> {
    let state = State::new(data.players.clone(), index, variant, data.options.clone());
    let mut game = Game::new(0, state, false, convention.build()?);
    game.catchup = true;

    let deck = &data.deck;
    let draw = |game: &mut Game, player_index: usize| -> Result<(), String> {
        let order = game.state.next_card_order;
        let id = (!(hide_own && player_index == index)).then(|| deck[order]);
        game.handle_action(&Action::Draw(DrawAction {
            player_index,
            order,
            id,
        }))
        .map_err(|e| e.to_string())
    };

    let n = game.state.num_players;
    for player_index in 0..n {
        for _ in 0..HAND_SIZE[n] {
            draw(&mut game, player_index)?;
        }
    }

    for perform in &data.actions {
        let player_index = game.state.current_player_index;
        let action = perform.to_action(&game.state, player_index, Some(deck), true);
        game.handle_action(&action).map_err(|e| e.to_string())?;

        if game.state.next_card_order < deck.len()
            && matches!(
                perform,
                PerformAction::Play { .. } | PerformAction::Discard { .. }
            )
        {
            draw(&mut game, player_index)?;
        }

        if matches!(perform, PerformAction::Play { .. }) && game.state.strikes == 3 {
            game.handle_action(&Action::GameOver(GameOverAction {
                end_condition: 0,
                player_index: Some(player_index),
            }))
            .map_err(|e| e.to_string())?;
        }

        let next = game
            .state
            .next_player_index(game.state.current_player_index);
        let turn = game.state.turn_count;
        game.handle_action(&Action::Turn(TurnAction {
            num: turn,
            current_player_index: Some(next),
        }))
        .map_err(|e| e.to_string())?;
    }

    game.catchup = false;
    Ok(game)
}

/// Common arguments of the replay and analysis tools.
pub struct ReplayArgs {
    pub data: GameData,
    pub convention: ConventionKind,
    pub variant: Arc<Variant>,
    pub args: std::collections::HashMap<String, String>,
}

pub fn parse_replay_args<S: AsRef<str>>(raw: &[S]) -> Result<ReplayArgs, String> {
    let args = parse_args(raw)?;
    let convention = args
        .get("convention")
        .and_then(|c| ConventionKind::parse(c).ok())
        .unwrap_or(ConventionKind::Reactor);

    let data = match (args.get("file"), args.get("id")) {
        (Some(file), _) => GameData::from_file(file)?,
        (None, Some(id)) => {
            let env = super::args::full_env(&args);
            let hostname = env
                .get("HANABI_HOSTNAME")
                .map_or("hanab.live", String::as_str);
            GameData::fetch_id(hostname, id)?
        }
        (None, None) => return Err("Must provide either id or file argument.".to_string()),
    };
    let variant = load_variant(&data.options.variant_name, &args)?;

    Ok(ReplayArgs {
        data,
        convention,
        variant,
        args,
    })
}
