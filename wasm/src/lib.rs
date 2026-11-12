//! The bot's analysis of a finished game, for the browser. Call [`analyze`] from a Web Worker: it blocks until it is done.

use std::sync::atomic::AtomicBool;

use hanab_bot::basics::Variant;
use hanab_bot::basics::game::{AnalysisOptions, AnalysisProgress, set_endgame_timeout_ms};
use hanab_bot::logger::{self, LogLevel};
use hanab_bot::settings::ConventionKind;
use hanab_bot::tools::game_data::{GameData, build_game};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Loads hanabi-live's `variants.json` and `suits.json`. Call once, before [`analyze`].
#[wasm_bindgen]
pub fn init_variants(variants_json: &str, colours_json: &str) -> Result<(), JsError> {
    Variant::init(variants_json, colours_json).map_err(|e| JsError::new(&e))
}

/// Lists the moves of a game (as hanab.live's `/export/<id>` gives it) that the bot would have played differently, as JSON:
/// `{"players": [...], "comments": [{"turn", "player", "mistake", "text"}]}`.
///
/// `convention` is `HGroup1` to `HGroup12`, `Reactor` or `RefSieve`. An endgame search that runs out of
/// `endgame_timeout_ms` (0: 500) counts as lost, so slow devices want more. `progress(movesDone, movesTotal)` is optional.
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

#[wasm_bindgen]
pub fn version() -> String {
    hanab_bot::settings::BOT_VERSION.to_string()
}

/// Whether the bot can play this variant and these table options.
#[wasm_bindgen]
pub fn is_game_supported(
    variant: &str,
    empty_clues: bool,
    detrimental_characters: bool,
    one_extra_card: bool,
    one_less_card: bool,
) -> bool {
    !hanab_bot::settings::is_unsupported_variant(variant)
        && hanab_bot::settings::unsupported_table_options(
            empty_clues,
            detrimental_characters,
            one_extra_card,
            one_less_card,
        )
        .is_empty()
}
