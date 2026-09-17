//! What the user chose for this connector, and the form they choose it in (03 §11 step 2).
//!
//! A default model per kind, and when that default applies. The defaults are the answer to "it
//! should just work without me naming a model": a call that leaves `model` out gets the model
//! *this user* picked for that kind. The rule beside them is the answer to the other half —
//! whether the chat model is allowed to name one at all, and in which modes.
//!
//! The options are live rather than written down: they are the models the user's own keys reach
//! today, read from the same catalogue `generate` chooses from, so the form cannot offer a model
//! the tool would then refuse. Unlike the tool's own list, the form offers models over a year
//! old as well, marked with their age — [`crate::models::MAX_AGE_DAYS`] is a rule about what a
//! *chat model* may reach for, not about what the user may choose.
//!
//! There used to be a spending ceiling here, added the day before and removed once the live
//! catalogue was read: 87 of the 101 media models on this machine publish no price at all, and
//! the ones that do publish a rate per token rather than a price per picture. A ceiling would
//! have refused nearly every generation while looking like a safety feature.

use std::collections::BTreeMap;

use gantry_core::{Mode, UserConfigField, UserConfigKind, UserConfigOption};

use crate::models::{Candidate, Kind, describe_kind, detail, kind_name};

/// The stored key per kind. Upper snake case, like every other connector's config keys.
pub const DEFAULT_IMAGE_MODEL: &str = "DEFAULT_IMAGE_MODEL";
pub const DEFAULT_SPEECH_MODEL: &str = "DEFAULT_SPEECH_MODEL";
pub const DEFAULT_VIDEO_MODEL: &str = "DEFAULT_VIDEO_MODEL";
/// When the default model applies at all.
pub const DEFAULT_MODEL_RULE: &str = "DEFAULT_MODEL_RULE";

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

/// When the user's default model is used, rather than the model the call named.
///
/// The middle one is the reason there is a setting at all rather than a fixed rule: in Auto
/// nobody is asked and no card appears, so the model's choice of model goes unseen — which is
/// exactly where somebody might want their own default to stand, while still letting the model
/// pick when they are there to look at the card.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Rule {
    /// Only when the call does not name a model. The default.
    #[default]
    Unnamed,
    /// That, and in Auto mode, where no card would have asked.
    Unattended,
    /// Always: the chat model never chooses the model.
    Always,
}

impl Rule {
    fn read(value: &str) -> Self {
        match value.trim() {
            "always" => Rule::Always,
            "unattended" => Rule::Unattended,
            _ => Rule::Unnamed,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Unnamed => "unnamed",
            Rule::Unattended => "unattended",
            Rule::Always => "always",
        }
    }

    /// Whether the user's default wins over a model the call named itself.
    #[must_use]
    pub fn overrides(self, mode: Mode) -> bool {
        match self {
            Rule::Unnamed => false,
            Rule::Unattended => mode == Mode::Auto,
            Rule::Always => true,
        }
    }
}

/// What the user chose, as the connector reads it back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preferences {
    /// `provider/model` per kind, where they named one.
    pub defaults: BTreeMap<&'static str, String>,
    pub rule: Rule,
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
        let rule = values
            .get(DEFAULT_MODEL_RULE)
            .map(|v| Rule::read(v))
            .unwrap_or_default();
        Self { defaults, rule }
    }

    /// The model this user wants for that kind, if any.
    #[must_use]
    pub fn default_for(&self, kind: Option<Kind>) -> Option<&str> {
        self.defaults.get(key_for(kind?)).map(String::as_str)
    }
}

/// The settings form, with the options filled in from the models this machine can reach.
///
/// `available` is already ranked newest-first, so the menu reads the way the automatic choice
/// works: the model at the top is the one that answers when nobody chooses.
#[must_use]
pub fn fields(available: &[Candidate], now_ms: i64) -> Vec<UserConfigField> {
    let mut fields: Vec<UserConfigField> = [Kind::Image, Kind::Speech, Kind::Video]
        .into_iter()
        .map(|kind| {
            let mut options = vec![UserConfigOption {
                value: AUTOMATIC.to_owned(),
                label: "Newest available".to_owned(),
                detail: available
                    .iter()
                    .find(|c| c.kind == kind && c.recent(now_ms))
                    .map(|c| format!("today: {}", c.key())),
            }];
            options.extend(available.iter().filter(|c| c.kind == kind).map(|c| {
                UserConfigOption {
                    value: c.key(),
                    label: c.key(),
                    // The age of an old one is said here rather than hidden: this menu is
                    // the one place a model over a year old can be chosen, and choosing one
                    // on purpose needs to look different from choosing one by accident.
                    detail: Some(match c.recent(now_ms) {
                        true => detail(c),
                        false => format!(
                            "{} \u{b7} {}",
                            detail(c),
                            c.age(now_ms).unwrap_or_else(|| "older".to_owned())
                        ),
                    }),
                }
            }));
            UserConfigField {
                key: key_for(kind).to_owned(),
                kind: UserConfigKind::Select,
                title: format!("Default model for {}", describe_kind(kind)),
                description: Some(format!(
                    "What a call uses when it does not name a model. The chat model can still \
                     name another one unless the rule below says otherwise; this is what it \
                     gets when it leaves `{}` out.",
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
        key: DEFAULT_MODEL_RULE.to_owned(),
        kind: UserConfigKind::Select,
        title: "When those defaults apply".to_owned(),
        description: Some(
            "A chat model may name a model of its own. This is how much notice to take of it."
                .to_owned(),
        ),
        required: false,
        sensitive: false,
        default: Some(Rule::Unnamed.as_str().to_owned()),
        options: vec![
            UserConfigOption {
                value: Rule::Unnamed.as_str().to_owned(),
                label: "Only when the chat model names none".to_owned(),
                detail: Some("it may pick; the card shows you which".to_owned()),
            },
            UserConfigOption {
                value: Rule::Unattended.as_str().to_owned(),
                label: "That, and always in Auto".to_owned(),
                detail: Some("where no card asks you anything".to_owned()),
            },
            UserConfigOption {
                value: Rule::Always.as_str().to_owned(),
                label: "Always \u{2014} my model, every time".to_owned(),
                detail: Some("whatever the chat model asks for".to_owned()),
            },
        ],
    });
    fields
}
