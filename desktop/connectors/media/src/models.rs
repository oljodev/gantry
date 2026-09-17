//! Which model makes the thing: finding the media models, naming one, and refusing an option
//! the model does not offer.
//!
//! Everything here reads the catalog the provider registry already keeps (02 §2) — the same
//! rows the model dialog shows. Nothing is hard-coded: a model list is a fact about somebody
//! else's service, and a copy of it in this crate would be out of date the week it was written.

use std::sync::Arc;

use gantry_core::{ArgChoice, ChoiceOption, MediaOptions, ProviderId};
use gantry_providers::{ModelInfo, ProviderRegistry, catalog, openai_chat::media::MediaRoute};
use gantry_store::Store;

/// What a media model makes. The routing's own enum, so "which endpoint" and "which kind" stay
/// one fact (02 §4b).
pub type Kind = MediaRoute;

/// How old a media model may be and still be something a **chat model** can pick: a year.
///
/// Olav's rule, and it is about who chooses rather than about the model: a chat model reaching
/// for an image model is reaching into its training data, where a name it remembers is as likely
/// to be two generations behind as current. The user's own menus show everything, marked with
/// its age, because choosing an older model on purpose is a different act from a model naming
/// one out of memory. On this machine the rule hides exactly one of 101 media models — it is a
/// guard against a bad habit, not a filter that does the choosing.
pub const MAX_AGE_DAYS: i64 = 365;

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

    /// What this model charges, in the unit its own provider publishes, or `None` where nobody
    /// published one.
    ///
    /// Measured against the live OpenRouter list on 2026-09-17, and it is worth writing down:
    /// **most media models publish no price at all** — 87 of the 101 on this machine — and the
    /// ones that do publish a *rate per token*, not a price per picture. So a price is
    /// something to show where it exists, not something to sort by, rank on, or promise a
    /// ceiling against; the first version of this connector did all three and the result was a
    /// menu of `$0.0000 a unit` with the only priced model at the top of it.
    #[must_use]
    pub fn price(&self) -> Option<String> {
        let p = self.info.pricing.as_ref()?;
        let rate = |v: f64| (v > 0.0).then(|| format!("${v:.2}"));
        match self.kind {
            Kind::Image => rate(p.image_output_per_mtok?).map(|r| format!("{r} / M drawn")),
            Kind::Speech => rate(p.output_per_mtok).map(|r| format!("{r} / M spoken")),
            Kind::Video => {
                let low = p
                    .video_per_second_usd
                    .values()
                    .copied()
                    .filter(|v| *v > 0.0)
                    .min_by(f64::total_cmp)?;
                Some(format!("${low:.2} a second"))
            }
        }
    }

    /// When the provider says the model was released, in milliseconds.
    #[must_use]
    pub fn released_at(&self) -> Option<i64> {
        self.info.created_at.map(|seconds| seconds * 1_000)
    }

    /// `Sep 2026`, for a menu somebody is choosing from. The single most useful thing to know
    /// about a media model, now that price turns out to be mostly unpublished.
    #[must_use]
    pub fn released(&self) -> Option<String> {
        let seconds = self.info.created_at?;
        let days = seconds.div_euclid(86_400);
        let (year, month, _) = civil_from_days(days);
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        Some(format!(
            "{} {year}",
            MONTHS[usize::try_from(month).unwrap_or(1).clamp(1, 12) - 1]
        ))
    }

    /// Whether this is one a chat model may pick (`MAX_AGE_DAYS`).
    ///
    /// A model whose provider published no date counts as recent: an unknown date is not proof
    /// of age, and refusing on one would hide models for a missing field rather than for being
    /// old. Every media model on this machine has a date, so this decides nothing today.
    #[must_use]
    pub fn recent(&self, now_ms: i64) -> bool {
        match self.released_at() {
            Some(released) => now_ms - released <= MAX_AGE_DAYS * 86_400_000,
            None => true,
        }
    }

    /// "18 months old", for saying why something is not on offer.
    #[must_use]
    pub fn age(&self, now_ms: i64) -> Option<String> {
        let released = self.released_at()?;
        let months = (now_ms - released) / (30 * 86_400_000);
        Some(match months {
            ..=1 => "less than a month old".to_owned(),
            m => format!("{m} months old"),
        })
    }
}

/// Days since the epoch to year, month, day. Howard Hinnant's `civil_from_days`, which is four
/// lines and exact, rather than a date crate for one label in one menu.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
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

/// Newest first, undated last, then by id.
///
/// The order *is* the automatic choice: a call with no model takes the first candidate of the
/// kind it asked for. It used to be cheapest-first, which the live catalogue disproved — 87 of
/// 101 media models publish no price at all, so "cheapest" meant "the one model that happened to
/// publish a number", which is how every picture in the first live run came from the same mini
/// model. Release date is published for all of them and is what a person actually reaches for.
/// Sorting by id last matters as much: without it the automatic choice would depend on the order
/// rows came out of SQLite, which is not a thing anybody could reproduce from a bill.
pub fn rank(models: &mut [Candidate]) {
    models.sort_by(|a, b| match (a.released_at(), b.released_at()) {
        (Some(x), Some(y)) => y.cmp(&x).then_with(|| a.model.cmp(&b.model)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.model.cmp(&b.model),
    });
}

/// Whether a name means this model: the whole key, the id the provider uses, or the last part of
/// that id.
///
/// The last part is what the first live run turned on. A chat model asked for `muse-image`; the
/// catalogue holds `openrouter/meta/muse-image`; the call was refused with "there is no media
/// model called `muse-image`" and the model — correctly — concluded it had invented a name.
/// It had not. A provider's ids carry a vendor prefix and a model writing the name of a model
/// does not.
#[must_use]
pub fn named_by(candidate: &Candidate, name: &str) -> bool {
    candidate.key() == name
        || candidate.model == name
        || candidate.model.rsplit('/').next() == Some(name)
}

/// The model to use out of the ones given: the one the call named, else the user's own default
/// for that kind, else the newest of it.
///
/// What is *in* `available` is the caller's decision and it matters: `pick` passes only the
/// models a chat model may use (`MAX_AGE_DAYS`), while the settings form passes everything.
/// `preferred` is the user's settings answer and is consulted when the call named nothing; a
/// preference that is no longer there falls through to the newest rather than failing, because
/// the user chose a model, not a promise that a provider would keep it.
pub fn choose(
    available: &[Candidate],
    named: Option<&str>,
    wanted: Option<Kind>,
    preferred: Option<&str>,
) -> Result<Candidate, String> {
    if let Some(named) = named {
        // The whole key first, then the id and its last part when that is unambiguous: a model
        // that writes `flux-1.1-pro` means the one model called that, and refusing it on a
        // technicality helps nobody.
        let exact: Vec<&Candidate> = available.iter().filter(|c| c.key() == named).collect();
        let bare: Vec<&Candidate> = available
            .iter()
            .filter(|c| c.key() != named && named_by(c, named))
            .collect();
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
            .find(|c| c.kind == wanted && named_by(c, preferred))
    {
        return Ok(found.clone());
    }
    available
        .iter()
        .find(|c| c.kind == wanted)
        .cloned()
        .ok_or_else(|| {
            format!(
                "No model that makes {} is available here. {}",
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
        "released".into(),
        match candidate.released() {
            Some(month) => month.into(),
            None => serde_json::Value::Null,
        },
    );
    // A labelled string rather than a number, because the number alone would be read as the
    // price of one picture and it is a rate per million tokens of one. Null where the provider
    // published nothing, which is most of them.
    row.insert(
        "price".into(),
        match candidate.price() {
            Some(price) => price.into(),
            None => serde_json::Value::Null,
        },
    );
    if is_default {
        row.insert("used_when_no_model_is_named".into(), true.into());
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

/// Everything `list_models` needs to answer: the catalogue, what to narrow it to, and what the
/// **tool** would actually do — which is the part that has to come from outside.
pub struct Listing<'a> {
    pub available: &'a [Candidate],
    pub now_ms: i64,
    pub kind: Option<Kind>,
    pub search: &'a str,
    pub limit: usize,
    /// What a call with no `model` would get, per kind, decided by [`pick`] rather than by this
    /// function. The first live run marked the *newest* model as the automatic one while the
    /// tool was using the user's own default, and the chat model noticed the contradiction and
    /// wrote a paragraph about it: a list that disagrees with the tool it describes is worse
    /// than no list.
    pub defaults: &'a [String],
    /// The user's settings say the chat model does not choose the model at all, so the list is
    /// for answering "what is there", not for picking from.
    pub fixed: bool,
}

/// What `list_models` answers: a sentence and a line per model for the model to read, and the
/// same list as JSON for it to act on.
///
/// Pure, and separate from the tool, so the thing under test is the filtering rather than a
/// connector holding a database. `available` is already ranked, so "newest first" is inherited
/// rather than re-decided — the list a call sees and the model a call gets are the same order.
#[must_use]
pub fn listing(l: &Listing<'_>) -> (String, serde_json::Value) {
    // Only what a chat model may pick (`MAX_AGE_DAYS`). The older ones are counted rather than
    // hidden in silence: a list that simply lacked them would have the model conclude a model it
    // remembers was never here.
    let older = l.available.len() - l.available.iter().filter(|c| c.recent(l.now_ms)).count();
    let available: Vec<Candidate> = l
        .available
        .iter()
        .filter(|c| c.recent(l.now_ms) || l.defaults.iter().any(|d| d == &c.key()))
        .cloned()
        .collect();
    let available = available.as_slice();

    let matching: Vec<&Candidate> = available
        .iter()
        .filter(|c| l.kind.is_none_or(|k| c.kind == k))
        .filter(|c| matches(c, l.search))
        .collect();
    let shown: Vec<&Candidate> = matching.iter().take(l.limit).copied().collect();
    let rows: Vec<serde_json::Value> = shown
        .iter()
        .map(|c| row(c, l.defaults.iter().any(|d| d == &c.key())))
        .collect();

    let mut summary = if matching.is_empty() {
        format!(
            "Nothing matches that. There are {} media models altogether; ask again without \
             `search`.",
            available.len()
        )
    } else if matching.len() == shown.len() {
        format!("{} models, newest first.", shown.len())
    } else {
        format!(
            "{} of {} models, newest first. Narrow it with `kind` or `search`, or raise \
             `limit`.",
            shown.len(),
            matching.len()
        )
    };
    if older > 0 {
        summary.push_str(&format!(
            " {older} more are over a year old and are not yours to pick; the user can still \
             choose one in this connector's settings."
        ));
    }
    if l.fixed {
        summary.push_str(
            " The user has fixed which model is used, so this list is for answering their \
             questions rather than for choosing from.",
        );
    }
    for candidate in &shown {
        summary.push_str(&format!(
            "\n{} \u{2014} {}, {}",
            candidate.key(),
            kind_name(candidate.kind),
            detail(candidate)
        ));
    }
    let structured = serde_json::json!({
        "models": rows,
        "shown": shown.len(),
        "matching": matching.len(),
        "available": available.len(),
        "hidden_as_too_old": older,
    });
    (summary, structured)
}

/// What to say about a model beside its name, in a menu or a list: when it came out, and what
/// it charges where anybody published that.
#[must_use]
pub fn detail(candidate: &Candidate) -> String {
    match (candidate.released(), candidate.price()) {
        (Some(released), Some(price)) => format!("{released} \u{b7} {price}"),
        (Some(released), None) => format!("{released} \u{b7} price not published"),
        (None, Some(price)) => price,
        (None, None) => "price not published".to_owned(),
    }
}

/// The model this call will actually use, and anything the user should be told about why.
///
/// One place decides it, because two would drift and the two are the permission card and the
/// call it is a card *for*: a card that named a different model from the one that then ran would
/// be worse than no card. Everything that makes the decision is here — the user's default, the
/// rule that says when the default beats a model the call named, and the year-old cutoff a chat
/// model may not reach past.
pub fn pick(
    all: &[Candidate],
    now_ms: i64,
    prefs: &crate::settings::Preferences,
    mode: gantry_core::Mode,
    named: Option<&str>,
    wanted: Option<Kind>,
) -> Result<Picked, String> {
    // What a chat model may reach for. The user's own default is looked up in the whole list
    // below, which is the point of the distinction.
    let offered: Vec<Candidate> = all.iter().filter(|c| c.recent(now_ms)).cloned().collect();
    let preferred = prefs.default_for(wanted);
    let preferred_candidate = preferred.and_then(|p| all.iter().find(|c| named_by(c, p)));

    // The user's own model, where their rule says it beats what the call asked for. The model is
    // told, because a tool that quietly ignored an argument would have it try the argument again.
    if let Some(candidate) = preferred_candidate
        && named.is_some()
        && prefs.rule.overrides(mode)
    {
        let note = format!(
            "The user's settings say to use {} for {} whatever a call asks for, so `model` was \
             ignored.",
            candidate.key(),
            describe_kind(candidate.kind)
        );
        return Ok(Picked {
            candidate: candidate.clone(),
            note: Some(note),
        });
    }

    if let Some(named) = named {
        // Refused for being old, rather than reported missing: a model that is told "there is no
        // such model" tries another name it remembers, where one told the real reason stops.
        if let Some(stale) = all.iter().find(|c| named_by(c, named) && !c.recent(now_ms)) {
            return Err(format!(
                "{} is {}, and Gantry only lets you pick media models released in the last \
                 year. {} The user can set an older one as their default in this connector's \
                 settings.",
                stale.key(),
                stale.age(now_ms).unwrap_or_else(|| "too old".to_owned()),
                offer(&offered, wanted.or(Some(stale.kind)))
            ));
        }
        let candidate = choose(&offered, Some(named), wanted, None)?;
        return Ok(Picked {
            candidate,
            note: None,
        });
    }

    // The user's default is looked up in the *whole* list, not in what a chat model may pick:
    // choosing an older model on purpose is the one way an older model is used at all.
    if let Some(candidate) = preferred_candidate.filter(|c| wanted.is_none_or(|k| c.kind == k)) {
        return Ok(Picked {
            candidate: candidate.clone(),
            note: None,
        });
    }
    let candidate = choose(&offered, None, wanted, None)?;
    // A default that is no longer there is worth a sentence rather than a silent swap: the user
    // chose that model once, and a bill from a different one should not be the first they hear
    // of it.
    let note = preferred
        .map(|p| format!("The user's default for this kind, {p}, is not available today."));
    Ok(Picked { candidate, note })
}

/// What [`pick`] decided, and what to say about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub candidate: Candidate,
    pub note: Option<String>,
}

/// The model as a permission card offers it (04 §7): the one this call would use, and the ones
#[must_use]
pub fn model_choice(
    all: &[Candidate],
    now_ms: i64,
    prefs: &crate::settings::Preferences,
    mode: gantry_core::Mode,
    named: Option<&str>,
    wanted: Option<Kind>,
) -> ArgChoice {
    let note;
    let picked = match pick(all, now_ms, prefs, mode, named, wanted) {
        Ok(picked) => {
            note = picked.note;
            Some(picked.candidate)
        }
        Err(_) => {
            // The card is not the place for the tool's error text, which is written for a model
            // reading a failure. What the user needs is one line saying why the name on the call
            // is not the name on the card.
            note = named.map(|named| match all.iter().find(|c| named_by(c, named)) {
                Some(stale) => format!(
                    "`{named}` is {}, so it is not one the chat model may pick.",
                    stale
                        .age(now_ms)
                        .unwrap_or_else(|| "over a year old".to_owned())
                ),
                None => format!("There is no `{named}` on this machine."),
            });
            None
        }
    };
    // A model that was refused still says what kind of thing this call was about, which is how
    // the menu stays about pictures when the picture model named was an old one.
    let kind = wanted.or(picked.as_ref().map(|c| c.kind)).or_else(|| {
        named.and_then(|named| all.iter().find(|c| named_by(c, named)).map(|c| c.kind))
    });
    // The menu is what a chat model may pick, plus whatever this call landed on — which can be
    // an older model, when that is the user's own default. With no kind and nothing usable it is
    // every model there is: picking one is what says which kind this call was about.
    let mut options: Vec<ChoiceOption> = all
        .iter()
        .filter(|c| c.recent(now_ms) || picked.as_ref().is_some_and(|p| p.key() == c.key()))
        .filter(|c| kind.is_none_or(|k| c.kind == k))
        .map(|c| ChoiceOption {
            value: c.key(),
            label: c.key(),
            detail: Some(match kind {
                Some(_) => detail(c),
                None => format!("{} \u{b7} {}", kind_name(c.kind), detail(c)),
            }),
        })
        .collect();
    let fallback = picked.clone().or_else(|| {
        choose(all, None, kind, None)
            .ok()
            .filter(|_| kind.is_some())
    });
    if let Some(chosen) = &fallback
        && !options.iter().any(|o| o.value == chosen.key())
    {
        options.insert(
            0,
            ChoiceOption {
                value: chosen.key(),
                label: chosen.key(),
                detail: Some(detail(chosen)),
            },
        );
    }
    ArgChoice {
        key: "model".to_owned(),
        label: "Model".to_owned(),
        value: fallback.map(|c| c.key()),
        options,
        note,
    }
}
