use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use serde_json::Value;

use super::card::{Identifiable, Identity};
use super::clue::{ClueKind, ClueLike};

pub const VARIANTS_URL: &str = "https://raw.githubusercontent.com/Hanabi-Live/hanabi-live/main/packages/game/src/json/variants.json";
pub const COLOURS_URL: &str = "https://raw.githubusercontent.com/Hanabi-Live/hanabi-live/main/packages/game/src/json/suits.json";

/// A set of name fragments; a name matches if it contains any of them.
///
/// (The Scala bot used unanchored regexes of alternated literals, which this replaces.)
#[derive(Clone, Copy, Debug)]
pub struct NamePattern(pub &'static [&'static str]);

impl NamePattern {
    pub fn matches(self, name: &str) -> bool {
        self.0.iter().any(|fragment| name.contains(fragment))
    }
}

pub const WHITISH: NamePattern = NamePattern(&["White", "Gray", "Light", "Null"]);
pub const RAINBOWISH: NamePattern = NamePattern(&["Rainbow", "Omni"]);
pub const PINKISH: NamePattern = NamePattern(&["Pink", "Omni"]);
pub const BROWNISH: NamePattern = NamePattern(&["Brown", "Muddy", "Cocoa", "Null"]);
pub const DARK: NamePattern = NamePattern(&["Black", "Dark", "Gray", "Cocoa"]);
pub const PRISM: NamePattern = NamePattern(&["Prism"]);
pub const MUDDY: NamePattern = NamePattern(&["Muddy", "Cocoa"]);
pub const INVERTED: NamePattern = NamePattern(&["Orange"]);
pub const NO_COLOUR: NamePattern =
    NamePattern(&["White", "Gray", "Light", "Null", "Rainbow", "Omni", "Prism"]);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SuitType {
    pub whitish: bool,
    pub rainbowish: bool,
    pub pinkish: bool,
    pub brownish: bool,
    pub dark: bool,
    pub prism: bool,
    pub muddy: bool,
    pub inverted: bool,
}

impl SuitType {
    pub fn from_name(name: &str) -> Self {
        Self {
            whitish: WHITISH.matches(name),
            rainbowish: RAINBOWISH.matches(name),
            pinkish: PINKISH.matches(name),
            brownish: BROWNISH.matches(name),
            dark: DARK.matches(name),
            prism: PRISM.matches(name),
            muddy: MUDDY.matches(name),
            inverted: INVERTED.matches(name),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Suit {
    pub name: String,
    pub abbreviation: Option<char>,
    pub suit_type: SuitType,
}

impl Suit {
    pub fn new(name: &str, abbreviation: Option<char>) -> Self {
        Self {
            name: name.to_string(),
            abbreviation,
            suit_type: SuitType::from_name(name),
        }
    }
}

/// The rule modifiers of a variant, other than its suits.
#[derive(Clone, Copy, Default, Debug)]
pub struct VariantFlags {
    pub critical_rank: Option<usize>,
    pub clue_starved: bool,
    pub special_rank: Option<usize>,
    /// The special rank touches all colours.
    pub rainbow_s: bool,
    /// The special rank touches no colours.
    pub white_s: bool,
    /// The special rank touches all ranks.
    pub pink_s: bool,
    /// The special rank touches no ranks.
    pub brown_s: bool,
    pub deceptive_s: bool,
    pub scarce_1s: bool,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub id: u32,
    pub name: String,
    pub suits: Vec<Suit>,
    pub short_forms: Vec<char>,
    /// Indices into `suits` of the suits that have their own colour clue, in order.
    pub colourable_suits: Vec<usize>,
    pub whitish: bool,
    pub rainbowish: bool,
    pub pinkish: bool,
    pub brownish: bool,
    pub dark: bool,
    pub prism: bool,
    pub muddy: bool,
    pub inverted: bool,
    pub critical_rank: Option<usize>,
    pub clue_starved: bool,
    pub special_rank: Option<usize>,
    pub rainbow_s: bool,
    pub white_s: bool,
    pub pink_s: bool,
    pub brown_s: bool,
    pub deceptive_s: bool,
    pub scarce_1s: bool,
}

#[derive(Default)]
struct Registry {
    variants: HashMap<String, Arc<Variant>>,
    colours: Vec<Suit>,
}

static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(Default::default);

impl Variant {
    /// Every identity in the variant, in ordinal order.
    pub fn all_ids(&self) -> impl Iterator<Item = Identity> + '_ {
        (0..self.suits.len())
            .flat_map(|suit_index| (1..=5).map(move |rank| Identity::new(suit_index, rank)))
    }

    pub fn touch_possibilities(&self, clue: &impl ClueLike) -> Vec<Identity> {
        self.all_ids()
            .filter(|&id| self.id_touched(id, clue))
            .collect()
    }

    pub fn card_count(&self, id: Identity) -> usize {
        if self.suits[id.suit_index].suit_type.dark || self.critical_rank == Some(id.rank) {
            1
        } else if id.rank == 1 && self.scarce_1s {
            2
        } else {
            [3, 2, 2, 2, 1][id.rank - 1]
        }
    }

    pub fn total_cards(&self) -> usize {
        self.all_ids().map(|id| self.card_count(id)).sum()
    }

    pub fn id_touched(&self, id: Identity, clue: &impl ClueLike) -> bool {
        let Identity { suit_index, rank } = id;
        let suit = &self.suits[suit_index];
        let value = clue.value();

        if clue.kind() == ClueKind::Colour {
            if suit.suit_type.rainbowish {
                return true;
            }
            if suit.suit_type.whitish {
                return false;
            }
            if self.special_rank == Some(rank) {
                if self.rainbow_s {
                    return true;
                }
                if self.white_s {
                    return false;
                }
            }
            if suit.suit_type.prism {
                return (rank - 1) % self.colourable_suits.len() == value;
            }
            self.colourable_suits.get(value) == Some(&suit_index)
        } else {
            if suit.suit_type.pinkish {
                return true;
            }
            if suit.suit_type.brownish {
                return false;
            }
            if self.special_rank == Some(rank) {
                if self.pink_s {
                    return rank != value;
                }
                if self.brown_s {
                    return false;
                }
                if self.deceptive_s {
                    return suit_index % 4 + if rank == 1 { 2 } else { 1 } == value;
                }
            }
            rank == value
        }
    }

    pub fn card_touched(&self, card: &impl Identifiable, clue: &impl ClueLike) -> bool {
        card.id().is_some_and(|id| self.id_touched(id, clue))
    }

    /// The name without a leading `Dark ` or `Light `.
    pub fn strip_name(name: &str) -> &str {
        name.strip_prefix("Dark ")
            .or_else(|| name.strip_prefix("Light "))
            .unwrap_or(name)
    }

    /// Loads the variant and suit definitions (the contents of [`VARIANTS_URL`] and [`COLOURS_URL`]).
    pub fn init(variants_json: &str, colours_json: &str) -> Result<(), String> {
        let colours_json: Value = serde_json::from_str(colours_json).map_err(|e| e.to_string())?;
        let variants_json: Value =
            serde_json::from_str(variants_json).map_err(|e| e.to_string())?;

        let colours: Vec<Suit> = colours_json
            .as_array()
            .ok_or("suits.json is not an array")?
            .iter()
            .map(|suit| {
                let name = suit["name"].as_str().ok_or("suit without a name")?;
                let abbreviation = suit["abbreviation"]
                    .as_str()
                    .and_then(|a| a.chars().next())
                    .map(|c| c.to_ascii_lowercase());
                Ok(Suit::new(name, abbreviation))
            })
            .collect::<Result<_, String>>()?;

        // Variants need the colours to be registered first.
        REGISTRY.write().unwrap().colours = colours;

        let variants = variants_json
            .as_array()
            .ok_or("variants.json is not an array")?
            .iter()
            .map(Variant::from_json)
            .map(|v| v.map(|v| (v.name.clone(), Arc::new(v))))
            .collect::<Result<HashMap<_, _>, String>>()?;

        REGISTRY.write().unwrap().variants = variants;
        Ok(())
    }

    /// Looks up a variant by name. [`Variant::init`] must have been called.
    pub fn get(name: &str) -> Result<Arc<Variant>, String> {
        REGISTRY
            .read()
            .unwrap()
            .variants
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Variant '{name}' not found! Were variants initialized?"))
    }

    pub fn from_json(json: &Value) -> Result<Self, String> {
        let flag = |key: &str| json[key].as_bool().unwrap_or(false);
        let rank = |key: &str| json[key].as_u64().map(|n| n as usize);

        let suit_names: Vec<&str> = json["suits"]
            .as_array()
            .ok_or("variant without suits")?
            .iter()
            .map(|s| s.as_str().ok_or("suit name is not a string"))
            .collect::<Result<_, _>>()?;

        Self::build(
            json["id"].as_u64().ok_or("variant without an id")? as u32,
            json["name"].as_str().ok_or("variant without a name")?,
            &suit_names,
            None,
            VariantFlags {
                critical_rank: rank("criticalRank"),
                clue_starved: flag("clueStarved"),
                special_rank: rank("specialRank"),
                rainbow_s: flag("specialRankAllClueColors"),
                white_s: flag("specialRankNoClueColors"),
                pink_s: flag("specialRankAllClueRanks"),
                brown_s: flag("specialRankNoClueRanks"),
                deceptive_s: flag("specialRankDeceptive"),
                scarce_1s: flag("scarceOnes"),
            },
        )
    }

    /// Builds a variant from its suit names.
    ///
    /// If `shorts` is given, those are the suit abbreviations; otherwise they are derived from the
    /// registered suit definitions.
    pub fn build(
        id: u32,
        name: &str,
        suit_names: &[&str],
        shorts: Option<&[char]>,
        flags: VariantFlags,
    ) -> Result<Self, String> {
        let mut suits = Vec::new();
        let mut short_forms = Vec::new();
        let mut colourable_suits = Vec::new();

        match shorts {
            Some(shorts) => {
                for (i, &suit_name) in suit_names.iter().enumerate() {
                    suits.push(Suit::new(suit_name, Some(shorts[i])));
                    short_forms.push(shorts[i]);
                    if !NO_COLOUR.matches(suit_name) {
                        colourable_suits.push(i);
                    }
                }
            }
            None => {
                let registry = REGISTRY.read().unwrap();
                for (i, &suit_name) in suit_names.iter().enumerate() {
                    let colour = registry.colours.iter().find(|c| c.name == suit_name);

                    let short = match suit_name {
                        "Black" => 'k',
                        "Pink" => 'i',
                        "Brown" => 'n',
                        _ => {
                            let abbreviation = colour
                                .and_then(|c| c.abbreviation)
                                .or_else(|| {
                                    suit_name.chars().next().map(|c| c.to_ascii_lowercase())
                                })
                                .ok_or("empty suit name")?;

                            if !short_forms.contains(&abbreviation) {
                                abbreviation
                            } else {
                                suit_name
                                    .to_lowercase()
                                    .chars()
                                    .find(|c| !short_forms.contains(c))
                                    .ok_or_else(|| {
                                        format!(
                                            "No unused character found for suit '{suit_name}' in {suit_names:?}!"
                                        )
                                    })?
                            }
                        }
                    };

                    suits.push(
                        colour
                            .cloned()
                            .unwrap_or_else(|| Suit::new(suit_name, Some(short))),
                    );
                    short_forms.push(short);
                    if !NO_COLOUR.matches(suit_name) {
                        colourable_suits.push(i);
                    }
                }
            }
        }

        let any = |f: fn(&SuitType) -> bool| suits.iter().any(|s| f(&s.suit_type));
        let VariantFlags {
            rainbow_s, pink_s, ..
        } = flags;

        Ok(Self {
            id,
            name: name.to_string(),
            whitish: any(|t| t.whitish),
            rainbowish: any(|t| t.rainbowish) || rainbow_s,
            pinkish: any(|t| t.pinkish) || pink_s,
            brownish: any(|t| t.brownish),
            dark: any(|t| t.dark),
            prism: any(|t| t.prism),
            muddy: any(|t| t.muddy),
            inverted: any(|t| t.inverted),
            critical_rank: flags.critical_rank,
            clue_starved: flags.clue_starved,
            special_rank: flags.special_rank,
            rainbow_s: flags.rainbow_s,
            white_s: flags.white_s,
            pink_s: flags.pink_s,
            brown_s: flags.brown_s,
            deceptive_s: flags.deceptive_s,
            scarce_1s: flags.scarce_1s,
            suits,
            short_forms,
            colourable_suits,
        })
    }

    /// Builds a variant with explicit abbreviations (used by tests).
    pub fn with_shorts(id: u32, name: &str, suit_names: &[&str], shorts: &[char]) -> Self {
        Self::build(id, name, suit_names, Some(shorts), VariantFlags::default())
            .expect("explicit shorts cannot fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basics::clue::BaseClue;

    fn no_variant() -> Variant {
        Variant::with_shorts(
            0,
            "No Variant",
            &["Red", "Yellow", "Green", "Blue", "Purple"],
            &['r', 'y', 'g', 'b', 'p'],
        )
    }

    #[test]
    fn counts_cards() {
        let v = no_variant();
        assert_eq!(v.total_cards(), 50);
        assert_eq!(v.card_count(Identity::new(0, 1)), 3);
        assert_eq!(v.card_count(Identity::new(0, 5)), 1);
    }

    #[test]
    fn touches_normal_suits() {
        let v = no_variant();
        let red = BaseClue::new(ClueKind::Colour, 0);
        let two = BaseClue::new(ClueKind::Rank, 2);
        assert!(v.id_touched(Identity::new(0, 3), &red));
        assert!(!v.id_touched(Identity::new(1, 3), &red));
        assert!(v.id_touched(Identity::new(4, 2), &two));
        assert_eq!(v.touch_possibilities(&red).len(), 5);
    }

    #[test]
    fn rainbow_is_touched_by_every_colour_and_white_by_none() {
        let rainbow = Variant::with_shorts(
            16,
            "Rainbow (5 Suits)",
            &["Red", "Yellow", "Green", "Blue", "Rainbow"],
            &['r', 'y', 'g', 'b', 'm'],
        );
        assert_eq!(rainbow.colourable_suits, [0, 1, 2, 3]);
        assert!(rainbow.id_touched(Identity::new(4, 1), &BaseClue::new(ClueKind::Colour, 2)));

        let white = Variant::with_shorts(
            22,
            "White (5 Suits)",
            &["Red", "Yellow", "Green", "Blue", "White"],
            &['r', 'y', 'g', 'b', 'w'],
        );
        assert!(!white.id_touched(Identity::new(4, 1), &BaseClue::new(ClueKind::Colour, 2)));
        assert!(white.id_touched(Identity::new(4, 1), &BaseClue::new(ClueKind::Rank, 1)));
    }

    #[test]
    fn prism_cycles_through_colours() {
        let prism = Variant::with_shorts(
            1465,
            "Prism (5 Suits)",
            &["Red", "Yellow", "Green", "Blue", "Prism"],
            &['r', 'y', 'g', 'b', 'i'],
        );
        let touched: Vec<_> = (1..=5)
            .filter(|&rank| {
                prism.id_touched(Identity::new(4, rank), &BaseClue::new(ClueKind::Colour, 0))
            })
            .collect();
        assert_eq!(touched, [1, 5]);
    }

    #[test]
    fn pink_and_brown_ranks() {
        let pink = Variant::with_shorts(
            107,
            "Pink (5 Suits)",
            &["Red", "Yellow", "Green", "Blue", "Pink"],
            &['r', 'y', 'g', 'b', 'i'],
        );
        assert!(pink.id_touched(Identity::new(4, 5), &BaseClue::new(ClueKind::Rank, 1)));
        let brown = Variant::with_shorts(
            70,
            "Brown (5 Suits)",
            &["Red", "Yellow", "Green", "Blue", "Brown"],
            &['r', 'y', 'g', 'b', 'n'],
        );
        assert!(!brown.id_touched(Identity::new(4, 5), &BaseClue::new(ClueKind::Rank, 5)));
    }

    #[test]
    fn black_suits_have_one_copy() {
        let black = Variant::with_shorts(
            21,
            "Black (5 Suits)",
            &["Red", "Yellow", "Green", "Blue", "Black"],
            &['r', 'y', 'g', 'b', 'k'],
        );
        assert_eq!(black.card_count(Identity::new(4, 1)), 1);
        assert_eq!(black.total_cards(), 40 + 5);
    }

    #[test]
    fn strips_dark_and_light() {
        assert_eq!(Variant::strip_name("Dark Brown"), "Brown");
        assert_eq!(Variant::strip_name("Light Pink"), "Pink");
        assert_eq!(Variant::strip_name("Red"), "Red");
    }

    #[test]
    fn parses_variant_json() {
        let json = serde_json::json!({
            "id": 5, "name": "Critical Fours", "suits": ["Red", "Yellow"], "criticalRank": 4, "clueStarved": true
        });
        let v = Variant::from_json(&json).unwrap();
        assert_eq!(v.critical_rank, Some(4));
        assert!(v.clue_starved);
        assert_eq!(v.card_count(Identity::new(0, 4)), 1);
        assert_eq!(v.short_forms, ['r', 'y']);
    }
}
