//! What the user chose for this connector, and the form they choose it in (03 §11 step 2).
//!
//! Three defaults and a ceiling. The defaults are the answer to "it should just work without me
//! naming a model": a call that leaves `model` out gets the model *this user* picked for that
//! kind, and only falls back to the cheapest when they have not picked one. The ceiling is the
//! answer to the other half of the same worry — this tool spends real money, and the amount is
//! decided by whatever the chat model typed.
//!
//! The options are live rather than written down: they are the models the user's own keys reach
//! today, read from the same catalogue `generate` chooses from, so the form cannot offer a model
//! the tool would then refuse.

use std::collections::BTreeMap;

use gantry_core::{UserConfigField, UserConfigKind, UserConfigOption};

use crate::models::{Candidate, Kind, describe_kind, kind_name};

/// The stored key per kind. Upper snake case, like every other connector's config keys.
pub const DEFAULT_IMAGE_MODEL: &str = "DEFAULT_IMAGE_MODEL";
pub const DEFAULT_SPEECH_MODEL: &str = "DEFAULT_SPEECH_MODEL";
pub const DEFAULT_VIDEO_MODEL: &str = "DEFAULT_VIDEO_MODEL";
/// The most one generation may cost, in US dollars. Empty means no ceiling.
pub const MAX_COST_USD: &str = "MAX_COST_USD";

/// The value that means "no model of my own, take the cheapest". A sentinel rather than an empty
/// string, because a menu whose first entry is blank reads as one nobody has answered yet.
pub const AUTOMATIC: &str = "auto";

#[must_use]
pub fn key_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => DEFAULT_IMAGE_MODEL,
        Kind::Speech => DEFAULT_SPEECH_MODEL,
        Kind::Video => DEFAULT_VIDEO_MODEL,
    }
}

/// What the user chose, as the connector reads it back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preferences {
    /// `provider/model` per kind, where they named one.
    pub defaults: BTreeMap<&'static str, String>,
    /// Dollars per generation, where they set a ceiling.
    pub ceiling: Option<f64>,
}

impl Preferences {
    /// Read from the stored answers. Anything unparseable is treated as unset: a settings row
    /// that cannot be read is not a reason to refuse to draw.
    #[must_use]
    pub fn read(values: &BTreeMap<String, String>) -> Self {
        let mut defaults = BTreeMap::new();
        for kind in [Kind::Image, Kind::Speech, Kind::Video] {
            let key = key_for(kind);
            let chosen = values.get(key).map(|v| v.trim()).unwrap_or_default();
            if !chosen.is_empty() && chosen != AUTOMATIC {
                defaults.insert(key, chosen.to_owned());
            }
        }
        let ceiling = values
            .get(MAX_COST_USD)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .and_then(|v| v.trim_start_matches('$').parse::<f64>().ok())
            .filter(|v| *v > 0.0);
        Self { defaults, ceiling }
    }

    /// The model this user wants for that kind, if any.
    #[must_use]
    pub fn default_for(&self, kind: Option<Kind>) -> Option<&str> {
        self.defaults.get(key_for(kind?)).map(String::as_str)
    }

    /// Whether this generation is inside the ceiling the user set.
    ///
    /// A model with no published price is refused while a ceiling stands, for the same reason it
    /// is never the automatic choice: a price nobody published cannot be shown to be under one.
    /// Refusing here costs nothing; finding out afterwards costs whatever it cost.
    pub fn affordable(&self, chosen: &Candidate) -> Result<(), String> {
        let Some(ceiling) = self.ceiling else {
            return Ok(());
        };
        match chosen.unit_cost() {
            Some(cost) if cost <= ceiling => Ok(()),
            Some(cost) => Err(format!(
                "{} costs about ${cost:.4} a unit, over the ${ceiling:.2} ceiling the user set \
                 for this connector. Pick a cheaper model — `list_models` has the prices — or \
                 ask them to raise the ceiling in the connector's settings.",
                chosen.key()
            )),
            None => Err(format!(
                "{} publishes no price, and the user set a ${ceiling:.2} ceiling for this \
                 connector, so it cannot be shown to be inside it. Pick a model with a price, \
                 which `list_models` marks.",
                chosen.key()
            )),
        }
    }
}

/// The settings form, with the options filled in from the models this machine can reach.
///
/// `available` is already ranked cheapest-first, so the menu reads the way the automatic choice
/// works: the model at the top is the one that answers when nobody chooses.
#[must_use]
pub fn fields(available: &[Candidate]) -> Vec<UserConfigField> {
    let mut fields: Vec<UserConfigField> =
        [Kind::Image, Kind::Speech, Kind::Video]
            .into_iter()
            .map(|kind| {
                let mut options = vec![UserConfigOption {
                    value: AUTOMATIC.to_owned(),
                    label: "Cheapest available".to_owned(),
                    detail: available
                        .iter()
                        .find(|c| c.kind == kind)
                        .map(|c| format!("today: {}", c.key())),
                }];
                options.extend(available.iter().filter(|c| c.kind == kind).map(|c| {
                    UserConfigOption {
                        value: c.key(),
                        label: c.key(),
                        detail: c.unit_cost().map(|cost| format!("${cost:.4} a unit")),
                    }
                }));
                UserConfigField {
                    key: key_for(kind).to_owned(),
                    kind: UserConfigKind::Select,
                    title: format!("Default model for {}", describe_kind(kind)),
                    description: Some(format!(
                        "What a call uses when it does not name a model. The chat model can still \
                     name another one; this is what it gets when it leaves `{}` out.",
                        kind_name(kind)
                    )),
                    required: false,
                    sensitive: false,
                    default: Some(AUTOMATIC.to_owned()),
                    options,
                }
            })
            .collect();
    fields.push(UserConfigField {
        key: MAX_COST_USD.to_owned(),
        kind: UserConfigKind::Number,
        title: "Most one generation may cost".to_owned(),
        description: Some(
            "In US dollars, at your provider's published price. A generation over this is \
             refused before anything is sent, and so is one whose model publishes no price. \
             Leave it empty for no ceiling."
                .to_owned(),
        ),
        required: false,
        sensitive: false,
        default: None,
        options: Vec::new(),
    });
    fields
}
