//! The bot's analysis of a finished game, for the browser.
//!
//! Build with `wasm/build.sh`, then call [`analyze`] from a Web Worker (it runs to the end on the calling thread, which can
//! take many seconds on a long game). A browser has no threads for us, so the players are replayed one after another.

use std::sync::atomic::AtomicBool;

use hanab_bot::basics::Variant;
use hanab_bot::basics::game::{AnalysisOptions, AnalysisProgress, set_endgame_timeout_ms};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Loads the variant tables: the text of hanabi-live's `variants.json` and `suits.json` (`packages/game/src/json`).
/// Call once, before [`analyze`].
#[wasm_bindgen]
pub fn init_variants(variants_json: &str, colours_json: &str) -> Result<(), JsError> {
    Variant::init(variants_json, colours_json).map_err(|e| JsError::new(&e))
}

/// Analyses a finished game: every move that is not what the bot would have done.
///
/// * `game_json`: the game as hanab.live exports it (`/export/<id>`): `players`, `deck` and `actions`.
/// * `variant`: the variant's name, e.g. `"No Variant"`.
/// * `convention`: `"HGroup"`, `"Reactor"` or `"RefSieve"` (with a level where it has one, e.g. `"HGroup3"`).
/// * `endgame_timeout_ms`: how long each endgame winrate may take (0 for the default, 500 ms). A search that runs out
///   of time counts as a winrate of 0, so on a slow device a longer limit avoids spurious "mistake" comments.
/// * `progress`: optional `(movesDone, movesTotal) => void`, called after every move examined.
///
/// Returns JSON text: `{"players": [...], "comments": [{"turn", "player", "mistake", "text"}]}`.
#[wasm_bindgen]
pub fn analyze(
    game_json: &str,
    variant: &str,
    convention: &str,
    endgame_timeout_ms: u32,
    progress: Option<js_sys::Function>,
) -> Result<String, JsError> {
    run(
        game_json,
        variant,
        convention,
        endgame_timeout_ms,
        progress.as_ref(),
    )
    .map_err(|e| JsError::new(&e))
}

fn run(
    game_json: &str,
    variant: &str,
    convention: &str,
    endgame_timeout_ms: u32,
    progress: Option<&js_sys::Function>,
) -> Result<String, String> {
    let export: serde_json::Value = serde_json::from_str(game_json).map_err(|e| e.to_string())?;
    let mut data = GameData::parse(&export)?;
    data.options.variant_name = variant.to_string();
    let variant = Variant::get(variant)?;
    let convention = ConventionKind::parse(convention)?;

    logger::set_default_level(LogLevel::Off);
    set_endgame_timeout_ms(u64::from(endgame_timeout_ms));

    // The JS function is only ever called from this thread (`threads: 1`), but the option wants `Sync`.
    struct OnThisThread<'a>(Option<&'a js_sys::Function>);
    unsafe impl Sync for OnThisThread<'_> {}
    let callback = OnThisThread(progress);
    let report = move |p: AnalysisProgress| {
        let callback = &callback;
        if let Some(f) = callback.0 {
            let _ = f.call2(
                &JsValue::NULL,
                &JsValue::from_f64(p.moves_done as f64),
                &JsValue::from_f64(p.moves_total as f64),
            );
        }
    };

    let cancel = AtomicBool::new(false);
    let options = AnalysisOptions {
        threads: 1,
        progress: Some(&report),
        cancel: Some(&cancel),
    };
    let game = build_game(convention, &data, variant, 0, false)?;
    let comments = game
        .analyze_comments_with(&options)
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "players": data.players,
        "comments": comments.iter().map(|c| json!({
            "turn": c.turn, "player": c.player_index, "mistake": c.mistake, "text": c.text,
        })).collect::<Vec<_>>(),
    })
    .to_string())
}

/// The bot version, e.g. `v1.3.0 (hanab-bot-rs)`.
#[wasm_bindgen]
pub fn version() -> String {
    hanab_bot::settings::BOT_VERSION.to_string()
}

/// Whether the bot can follow games with this variant and these table options: false for the variants it knows it cannot
/// play (Blind, Mute, Up or Down, ...) and for the options it can't handle. The lists are in the bot's `client.rs`, so
/// that the page does not need a copy of them.
#[wasm_bindgen]
pub fn is_game_supported(
    variant: &str,
    empty_clues: bool,
    detrimental_characters: bool,
    one_extra_card: bool,
    one_less_card: bool,
) -> bool {
    !hanab_bot::client::is_unsupported_variant(variant)
        && hanab_bot::client::unsupported_table_options(
            empty_clues,
            detrimental_characters,
            one_extra_card,
            one_less_card,
        )
        .is_empty()
}
