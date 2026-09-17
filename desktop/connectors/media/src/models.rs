//! Which model makes the thing: finding the media models, naming one, and refusing an option
//! the model does not offer.
//!
//! Everything here reads the catalog the provider registry already keeps (02 §2) — the same
//! rows the model dialog shows. Nothing is hard-coded: a model list is a fact about somebody
//! else's service, and a copy of it in this crate would be out of date the week it was written.

use std::sync::Arc;

use gantry_core::{MediaOptions, ProviderId};
use gantry_providers::{ModelInfo, ProviderRegistry, catalog, openai_chat::media::MediaRoute};
use gantry_store::Store;

/// What a media model makes. The routing's own enum, so "which endpoint" and "which kind" stay
/// one fact (02 §4b).
pub type Kind = MediaRoute;

/// How many models an error message names before it stops. A model that asked for something
/// impossible needs a few real ids to pick from, not the whole catalog.
pub const NAMED_IN_AN_ERROR: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub provider: ProviderId,
    pub model: String,
    pub kind: Kind,
    pub info: ModelInfo,
}

impl Candidate {
    /// `provider/model`, which is how a model names one and how `model_options` is keyed.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }

    /// One comparable number per model, for picking a default: what one unit of output costs.
    /// `None` where the provider published nothing, and a model with no price is never the
    /// automatic choice — an unknown price is the one that cannot be defended afterwards.
    #[must_use]
    pub fn unit_cost(&self) -> Option<f64> {
        let p = self.info.pricing.as_ref()?;
        match self.kind {
            Kind::Image => p
                .image_output_usd
                .or(p.request_usd)
                .or(Some(p.output_per_mtok).filter(|v| *v > 0.0)),
            Kind::Speech => Some(p.output_per_mtok).filter(|v| *v > 0.0),
            Kind::Video => p
                .video_per_second_usd
                .values()
                .copied()
                .filter(|v| *v > 0.0)
                .min_by(|a, b| a.total_cmp(b)),
        }
    }
}

/// "image", "speech" or "video", as the tool's arguments spell it.
#[must_use]
pub fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => "image",
        Kind::Speech => "speech",
        Kind::Video => "video",
    }
}

/// The same thing as a noun phrase, for a sentence a person reads.
#[must_use]
pub fn describe_kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => "a picture",
        Kind::Speech => "spoken audio",
        Kind::Video => "a video clip",
    }
}

pub fn parse_kind(name: &str) -> Result<Kind, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "image" | "picture" => Ok(Kind::Image),
        "speech" | "audio" | "voice" => Ok(Kind::Speech),
        "video" | "clip" => Ok(Kind::Video),
        other => Err(format!(
            "`kind` is `image`, `speech` or `video`, not `{other}`."
        )),
    }
}

/// Every media model the user could actually be charged for right now: in the cached catalog,
/// on a provider that is configured and has a key.
#[must_use]
pub fn list(providers: &Arc<ProviderRegistry>, store: &Arc<Store>) -> Vec<Candidate> {
    let mut out = Vec::new();
    for id in providers.ids() {
        let Some(provider) = providers.get(&id) else {
            continue;
        };
        if !provider.has_key() {
            continue;
        }
        let kind = provider.kind();
        let Ok(models) = catalog::cached(store, id.as_str(), kind) else {
            continue;
        };
        for info in models {
            let Some(route) = gantry_providers::openai_chat::media::route(Some(&info)) else {
                continue;
            };
            out.push(Candidate {
                provider: id.clone(),
                model: info.id.clone(),
                kind: route,
                info,
            });
        }
    }
    rank(&mut out);
    out
}

/// Cheapest first, unpriced last, then by id.
///
/// The order *is* the default: `choose` takes the first candidate of the kind it was asked for.
/// Sorting by id last matters as much as the price does — without it the automatic choice would
/// depend on the order rows came out of SQLite, which is not a thing anybody could reproduce
/// from a bill.
pub fn rank(models: &mut [Candidate]) {
    models.sort_by(|a, b| match (a.unit_cost(), b.unit_cost()) {
        (Some(x), Some(y)) => x.total_cmp(&y).then_with(|| a.model.cmp(&b.model)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.model.cmp(&b.model),
    });
}

/// The model to use: the one the call named, else the user's own default for that kind, else
/// the cheapest of it.
///
/// `preferred` is the user's settings answer (`settings.rs`) and is only consulted when the call
/// named nothing — a model the chat model asked for by name is not quietly replaced by a
/// setting. A preference that is no longer available falls through to the cheapest rather than
/// failing: the user chose a model, not a promise that a provider would keep it.
pub fn choose(
    available: &[Candidate],
    named: Option<&str>,
    wanted: Option<Kind>,
    preferred: Option<&str>,
) -> Result<Candidate, String> {
    if let Some(named) = named {
        // `provider/model` first, then a bare model id when it is unambiguous: a model that
        // writes `flux-1.1-pro` means the one model called that, and refusing it on a
        // technicality helps nobody.
        let exact: Vec<&Candidate> = available.iter().filter(|c| c.key() == named).collect();
        let bare: Vec<&Candidate> = available.iter().filter(|c| c.model == named).collect();
        let found = match (exact.as_slice(), bare.as_slice()) {
            ([one], _) | ([], [one]) => *one,
            ([], []) => {
                return Err(format!(
                    "There is no media model called `{named}` on a provider with a key. {}",
                    offer(available, wanted)
                ));
            }
            ([], many) => {
                let names: Vec<String> = many.iter().map(|c| c.key()).collect();
                return Err(format!(
                    "`{named}` is on more than one provider: {}. Name one of those.",
                    names.join(", ")
                ));
            }
            _ => unreachable!("an exact key matches at most one provider and model"),
        };
        if let Some(wanted) = wanted
            && found.kind != wanted
        {
            return Err(format!(
                "{} makes {}, not {}.",
                found.key(),
                describe_kind(found.kind),
                describe_kind(wanted)
            ));
        }
        return Ok(found.clone());
    }

    let Some(wanted) = wanted else {
        return Err(format!(
            "Say what to make: `kind` is `image`, `speech` or `video`, or name a `model`. {}",
            offer(available, None)
        ));
    };
    if let Some(preferred) = preferred
        && let Some(found) = available
            .iter()
            .find(|c| c.kind == wanted && (c.key() == preferred || c.model == preferred))
    {
        return Ok(found.clone());
    }
    available
        .iter()
        .find(|c| c.kind == wanted)
        .cloned()
        .ok_or_else(|| {
            format!(
                "No model that makes {} is available. {}",
                describe_kind(wanted),
                offer(available, None)
            )
        })
}

/// What to say after a refusal: the models there actually are, so the next call can work.
fn offer(available: &[Candidate], kind: Option<Kind>) -> String {
    let matching: Vec<&Candidate> = available
        .iter()
        .filter(|c| kind.is_none_or(|k| c.kind == k))
        .collect();
    if matching.is_empty() {
        return "This machine has no media models at all: the user needs a provider key, and \
                the model list refreshed in Settings → Providers."
            .to_owned();
    }
    let names: Vec<String> = matching
        .iter()
        .take(NAMED_IN_AN_ERROR)
        .map(|c| format!("{} ({})", c.key(), kind_name(c.kind)))
        .collect();
    let more = matching.len().saturating_sub(names.len());
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!("Available: {}{tail}.", names.join(", "))
}

/// An option the model does not offer is an error rather than a near miss (02 §4b): the
/// endpoint would refuse it, and this says so before the money is spent.
pub fn check(chosen: &Candidate, options: &MediaOptions) -> Result<(), String> {
    let caps = &chosen.info.capabilities;
    let one = |field: &str, value: Option<&String>, allowed: &[String]| -> Result<(), String> {
        let (Some(value), false) = (value, allowed.is_empty()) else {
            return Ok(());
        };
        if allowed.iter().any(|a| a == value) {
            return Ok(());
        }
        Err(format!(
            "{} does not offer `{field}` {value}. It offers: {}.",
            chosen.key(),
            allowed.join(", ")
        ))
    };
    one(
        "aspect_ratio",
        options.aspect_ratio.as_ref(),
        &caps.aspect_ratios,
    )?;
    one("resolution", options.resolution.as_ref(), &caps.resolutions)?;
    one("quality", options.quality.as_ref(), &caps.qualities)?;
    one("voice", options.voice.as_ref(), &caps.voices)?;
    if let Some(seconds) = options.duration_seconds
        && !caps.durations.is_empty()
        && !caps.durations.contains(&seconds)
    {
        let allowed: Vec<String> = caps.durations.iter().map(u32::to_string).collect();
        return Err(format!(
            "{} does not make {seconds}-second clips. It makes: {} seconds.",
            chosen.key(),
            allowed.join(", ")
        ));
    }
    Ok(())
}

/// One model as `list_models` reports it: the id a later call would name, what it costs, and
/// the options it offers.
///
/// The capability lists are the provider's own spellings, copied rather than normalised — they
/// are what `check` compares against, so a list that read differently here would be a second
/// version of the truth, and the model would be told one thing and refused by another.
#[must_use]
pub fn row(candidate: &Candidate, is_default: bool) -> serde_json::Value {
    let caps = &candidate.info.capabilities;
    let mut row = serde_json::Map::new();
    row.insert("id".into(), candidate.key().into());
    row.insert("kind".into(), kind_name(candidate.kind).into());
    row.insert(
        "unit_cost_usd".into(),
        match candidate.unit_cost() {
            Some(cost) => serde_json::json!(cost),
            None => serde_json::Value::Null,
        },
    );
    if is_default {
        row.insert("default_without_a_model".into(), true.into());
    }
    let mut list = |key: &str, values: &[String]| {
        if !values.is_empty() {
            row.insert(key.to_owned(), serde_json::json!(values));
        }
    };
    list("aspect_ratios", &caps.aspect_ratios);
    list("resolutions", &caps.resolutions);
    list("qualities", &caps.qualities);
    list("voices", &caps.voices);
    if !caps.durations.is_empty() {
        row.insert(
            "durations_seconds".into(),
            serde_json::json!(caps.durations),
        );
    }
    serde_json::Value::Object(row)
}

/// Whether a model matches what a search asked for: the words are looked for in the id, all of
/// them, in any order, case-insensitively. A search nobody typed matches everything.
#[must_use]
pub fn matches(candidate: &Candidate, search: &str) -> bool {
    let key = candidate.key().to_ascii_lowercase();
    search
        .split_whitespace()
        .all(|word| key.contains(&word.to_ascii_lowercase()))
}

/// What `list_models` answers: a sentence and a line per model for the model to read, and the
/// same list as JSON for it to act on.
///
/// Pure, and separate from the tool, so the thing under test is the filtering rather than a
/// connector holding a database. `available` is already ranked, so "cheapest first" is inherited
/// rather than re-decided — the list a call sees and the model a call gets are the same order.
#[must_use]
pub fn listing(
    available: &[Candidate],
    kind: Option<Kind>,
    search: &str,
    limit: usize,
) -> (String, serde_json::Value) {
    // What a call with no `model` would pick, marked in the list, so the model can see that
    // leaving it out is a real answer rather than a gap it has to fill.
    let defaults: Vec<String> = [Kind::Image, Kind::Speech, Kind::Video]
        .into_iter()
        .filter_map(|k| available.iter().find(|c| c.kind == k))
        .map(Candidate::key)
        .collect();
    let matching: Vec<&Candidate> = available
        .iter()
        .filter(|c| kind.is_none_or(|k| c.kind == k))
        .filter(|c| matches(c, search))
        .collect();
    let shown: Vec<&Candidate> = matching.iter().take(limit).copied().collect();
    let rows: Vec<serde_json::Value> = shown
        .iter()
        .map(|c| row(c, defaults.contains(&c.key())))
        .collect();

    let mut summary = if matching.is_empty() {
        format!(
            "Nothing matches that. There are {} media models altogether; ask again without \
             `search`.",
            available.len()
        )
    } else if matching.len() == shown.len() {
        format!("{} models, cheapest first.", shown.len())
    } else {
        format!(
            "{} of {} models, cheapest first. Narrow it with `kind` or `search`, or raise \
             `limit`.",
            shown.len(),
            matching.len()
        )
    };
    for candidate in &shown {
        summary.push_str(&format!(
            "\n{} \u{2014} {}, {}",
            candidate.key(),
            kind_name(candidate.kind),
            match candidate.unit_cost() {
                Some(cost) => format!("${cost:.4} a unit"),
                None => "no published price".to_owned(),
            }
        ));
    }
    let structured = serde_json::json!({
        "models": rows,
        "shown": shown.len(),
        "matching": matching.len(),
        "available": available.len(),
    });
    (summary, structured)
}
