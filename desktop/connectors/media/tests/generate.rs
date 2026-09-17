//! Choosing the model, and refusing before the money is spent.
//!
//! The endpoints themselves are M4's and are tested in `gantry-providers`; what is new here is
//! everything that happens before one is called. None of it needs a network, which is the point:
//! every refusal this connector can make, it makes for free.

use std::collections::BTreeMap;

use gantry_connector_media::{
    AUTOMATIC, Candidate, DEFAULT_MODEL_RULE, Kind, MANIFEST, Preferences, Rule, choose,
    definitions, key_for, kind_name, listing, model_choice, parse_kind, pick, rank, settings_form,
};
use gantry_core::{MediaOptions, Mode, ProviderId, RiskTier};
use gantry_providers::{ModelCapabilities, ModelInfo, Pricing, provider::Modality};

/// A fixed "now", so a test about a model's age is about the model rather than about the day it
/// is run. 2026-09-17, the day the year-old rule was asked for.
const NOW: i64 = 1_789_603_200_000;
const MONTH: i64 = 30 * 86_400_000;

fn model(id: &str, out: Modality, price: Option<f64>, months_ago: i64) -> ModelInfo {
    let capabilities = ModelCapabilities {
        output: vec![out],
        ..ModelCapabilities::default()
    };
    ModelInfo {
        id: id.to_owned(),
        display_name: id.to_owned(),
        created_at: Some((NOW - months_ago * MONTH) / 1_000),
        context_window: None,
        max_output: None,
        // Each kind is priced where its own endpoint publishes it (02 §4b): an image and speech
        // by the token, video by the second. A helper that put every price in one field would
        // make the price label look like it worked when it did not.
        pricing: price.map(|p| Pricing {
            input_per_mtok: 0.0,
            output_per_mtok: if out == Modality::Speech { p } else { 0.0 },
            cache_read_per_mtok: None,
            image_input_usd: None,
            image_output_per_mtok: (out == Modality::Image).then_some(p),
            request_usd: None,
            audio_input_per_mtok: None,
            audio_output_per_mtok: None,
            video_per_second_usd: if out == Modality::Video {
                BTreeMap::from([("720p".to_owned(), p)])
            } else {
                BTreeMap::new()
            },
        }),
        capabilities,
    }
}

fn candidate(provider: &str, id: &str, kind: Kind, price: Option<f64>) -> Candidate {
    aged(provider, id, kind, price, 1)
}

fn aged(provider: &str, id: &str, kind: Kind, price: Option<f64>, months_ago: i64) -> Candidate {
    let out = match kind {
        Kind::Image => Modality::Image,
        Kind::Speech => Modality::Speech,
        Kind::Video => Modality::Video,
    };
    Candidate {
        provider: ProviderId::new(provider.to_owned()),
        model: id.to_owned(),
        kind,
        info: model(id, out, price, months_ago),
    }
}

/// Ranked, like the real list: newest first. `meta/muse-image` carries a vendor prefix, which is
/// what the catalogue actually looks like and what the first live run broke on.
fn catalogue() -> Vec<Candidate> {
    let mut all = vec![
        aged("openrouter", "new-draw", Kind::Image, Some(30.0), 1),
        aged("openrouter", "meta/muse-image", Kind::Image, None, 4),
        aged("openrouter", "mid-draw", Kind::Image, Some(8.0), 7),
        aged("openrouter", "old-draw", Kind::Image, Some(3.0), 20),
        aged("openrouter", "speak", Kind::Speech, Some(20.0), 2),
        aged("xai", "new-draw", Kind::Image, Some(12.0), 3),
    ];
    rank(&mut all);
    all
}

fn prefs(pairs: &[(&str, &str)]) -> Preferences {
    Preferences::read(
        &pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

fn picked(named: Option<&str>, kind: Option<Kind>) -> Result<String, String> {
    pick(
        &catalogue(),
        NOW,
        &Preferences::default(),
        Mode::Manual,
        named,
        kind,
    )
    .map(|p| p.candidate.key())
}

#[test]
fn the_generating_tool_is_an_external_write_and_never_runs_beside_itself() {
    let defs = definitions();
    assert_eq!(defs[0].name, "generate");
    assert_eq!(defs[0].tier, RiskTier::WriteExternal);
    assert!(
        !defs[0].parallel_safe,
        "two generations at once are two charges decided as one"
    );
    let schema = &defs[0].input_schema;
    assert_eq!(schema["required"], serde_json::json!(["prompt"]));
    assert!(
        schema["properties"].get("n").is_none() && schema["properties"].get("count").is_none(),
        "there is no way to ask for more than one thing per call"
    );
}

/// Two tools, and only one of them can spend anything. Reading the catalogue is a local read,
/// so a mode that asks before every external write does not ask before this.
#[test]
fn looking_at_the_list_costs_nothing() {
    let defs = definitions();
    assert_eq!(defs.len(), 2);
    let list = &defs[1];
    assert_eq!(list.name, "list_models");
    assert_eq!(list.tier, RiskTier::Read);
    assert!(list.parallel_safe);
    assert!(
        list.input_schema.get("required").is_none(),
        "every argument is a way to narrow it; none of them is needed"
    );
}

#[test]
fn a_kind_is_spelled_the_way_a_person_would() {
    assert_eq!(parse_kind("image").unwrap(), Kind::Image);
    assert_eq!(parse_kind(" Picture ").unwrap(), Kind::Image);
    assert_eq!(parse_kind("speech").unwrap(), Kind::Speech);
    assert_eq!(parse_kind("voice").unwrap(), Kind::Speech);
    assert_eq!(parse_kind("video").unwrap(), Kind::Video);
    let err = parse_kind("hologram").unwrap_err();
    assert!(err.contains("hologram") && err.contains("image"), "{err}");
}

/// The order is the automatic choice, so this is the test that says what the default is.
///
/// It is release date and not price: 87 of the 101 media models on the machine this was written
/// on publish no price at all, so "cheapest" meant "the one that happened to publish a number".
#[test]
fn the_order_is_newest_first_and_undated_last() {
    let mut models = vec![
        aged("openrouter", "ancient", Kind::Image, Some(1.0), 30),
        aged("openrouter", "fresh", Kind::Image, None, 1),
        aged("openrouter", "middling", Kind::Image, Some(50.0), 6),
    ];
    let mut undated = aged("openrouter", "nobody-says", Kind::Image, None, 0);
    undated.info.created_at = None;
    models.push(undated);
    rank(&mut models);
    let order: Vec<&str> = models.iter().map(|c| c.model.as_str()).collect();
    assert_eq!(order, ["fresh", "middling", "ancient", "nobody-says"]);
}

#[test]
fn without_a_model_the_newest_of_that_kind_answers() {
    assert_eq!(
        picked(None, Some(Kind::Image)).unwrap(),
        "openrouter/new-draw"
    );
    assert_eq!(
        picked(None, Some(Kind::Speech)).unwrap(),
        "openrouter/speak"
    );
}

/// The bug the first live run found. A chat model asked for `muse-image`; the catalogue holds
/// `openrouter/meta/muse-image`; it was told there was no such model, and concluded — reasonably
/// — that it had invented the name. It had not.
#[test]
fn the_last_part_of_an_id_names_the_model() {
    assert_eq!(
        picked(Some("muse-image"), Some(Kind::Image)).unwrap(),
        "openrouter/meta/muse-image"
    );
    assert_eq!(
        picked(Some("meta/muse-image"), None).unwrap(),
        "openrouter/meta/muse-image"
    );
    assert_eq!(
        picked(Some("openrouter/meta/muse-image"), None).unwrap(),
        "openrouter/meta/muse-image"
    );
}

#[test]
fn a_bare_model_id_on_two_providers_asks_which() {
    let err = picked(Some("new-draw"), None).unwrap_err();
    assert!(err.contains("openrouter/new-draw"), "{err}");
    assert!(err.contains("xai/new-draw"), "{err}");
}

#[test]
fn a_full_key_beats_the_ambiguity() {
    assert_eq!(picked(Some("xai/new-draw"), None).unwrap(), "xai/new-draw");
}

/// The error is the next call's instructions: a model that named something that is not there
/// gets real ids back rather than "not found".
#[test]
fn an_unknown_model_is_refused_with_what_there_is() {
    let err = picked(Some("dall-e-9"), Some(Kind::Image)).unwrap_err();
    assert!(err.contains("dall-e-9"), "{err}");
    assert!(err.contains("openrouter/new-draw"), "{err}");
    assert!(
        !err.contains("openrouter/speak"),
        "a picture question is not answered with a voice model: {err}"
    );
}

/// Olav's rule: a chat model may not reach for a model over a year old, because a name it
/// remembers is as likely to be two generations behind as current. It is told the real reason,
/// so it stops rather than trying another name it half-remembers.
#[test]
fn a_model_over_a_year_old_is_not_the_chat_models_to_pick() {
    let err = picked(Some("old-draw"), Some(Kind::Image)).unwrap_err();
    assert!(err.contains("20 months old"), "{err}");
    assert!(err.contains("last year"), "{err}");
    assert!(err.contains("settings"), "{err}");
    assert!(err.contains("openrouter/new-draw"), "{err}");
}

/// The same model, chosen by the user in the settings form, is used without complaint. The rule
/// is about who is choosing, not about the model.
#[test]
fn the_user_may_still_make_an_older_model_their_default() {
    let chosen = pick(
        &catalogue(),
        NOW,
        &prefs(&[(key_for(Kind::Image), "openrouter/old-draw")]),
        Mode::Manual,
        None,
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(chosen.candidate.key(), "openrouter/old-draw");
    assert_eq!(chosen.note, None);
}

#[test]
fn a_model_that_makes_the_wrong_thing_is_refused_before_it_is_paid_for() {
    let err = picked(Some("openrouter/speak"), Some(Kind::Image)).unwrap_err();
    assert!(
        err.contains("spoken audio") && err.contains("a picture"),
        "{err}"
    );
}

#[test]
fn no_model_and_no_kind_is_a_question_rather_than_a_guess() {
    let err = picked(None, None).unwrap_err();
    assert!(err.contains("`kind`"), "{err}");
}

#[test]
fn a_kind_nobody_can_make_says_so() {
    let err = picked(None, Some(Kind::Video)).unwrap_err();
    assert!(err.contains("a video clip"), "{err}");
    assert!(err.contains("Available:"), "{err}");
}

#[test]
fn with_nothing_at_all_the_error_names_the_setting_that_fixes_it() {
    let err = choose(&[], None, Some(Kind::Image), None).unwrap_err();
    assert!(err.contains("Settings"), "{err}");
}

// ---------------------------------------------------------------- the user's own settings

#[test]
fn a_default_model_answers_a_call_that_names_none() {
    let chosen = pick(
        &catalogue(),
        NOW,
        &prefs(&[(key_for(Kind::Image), "openrouter/mid-draw")]),
        Mode::Manual,
        None,
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(chosen.candidate.key(), "openrouter/mid-draw");
}

/// The default rule: a model the call named is the model that runs, and the setting is what
/// happens when nobody said anything.
#[test]
fn by_default_a_named_model_beats_the_users_default() {
    let chosen = pick(
        &catalogue(),
        NOW,
        &prefs(&[(key_for(Kind::Image), "openrouter/mid-draw")]),
        Mode::Manual,
        Some("openrouter/new-draw"),
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(chosen.candidate.key(), "openrouter/new-draw");
}

/// "Always": the chat model never chooses. It is told its argument was ignored, because a tool
/// that quietly drops one has the model try it again next turn.
#[test]
fn the_always_rule_makes_the_users_model_the_only_model() {
    let chosen = pick(
        &catalogue(),
        NOW,
        &prefs(&[
            (key_for(Kind::Image), "openrouter/mid-draw"),
            (DEFAULT_MODEL_RULE, Rule::Always.as_str()),
        ]),
        Mode::Manual,
        Some("openrouter/new-draw"),
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(chosen.candidate.key(), "openrouter/mid-draw");
    let note = chosen.note.expect("the model is told");
    assert!(note.contains("ignored"), "{note}");
}

/// "In Auto too": the mode where nobody is asked and no card appears is exactly where a user
/// might want their own model to stand, while still letting the model pick when they can see it.
#[test]
fn the_unattended_rule_only_binds_where_no_card_would_ask() {
    let settings = prefs(&[
        (key_for(Kind::Image), "openrouter/mid-draw"),
        (DEFAULT_MODEL_RULE, Rule::Unattended.as_str()),
    ]);
    let in_auto = pick(
        &catalogue(),
        NOW,
        &settings,
        Mode::Auto,
        Some("openrouter/new-draw"),
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(in_auto.candidate.key(), "openrouter/mid-draw");

    let watched = pick(
        &catalogue(),
        NOW,
        &settings,
        Mode::AutoEdit,
        Some("openrouter/new-draw"),
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(watched.candidate.key(), "openrouter/new-draw");
}

/// A provider key removed, or a model retired: the preference stops being available and the
/// newest answers rather than the call failing over a setting — with a sentence, because a bill
/// from a different model should not be the first the user hears of it.
#[test]
fn a_default_that_is_gone_falls_through_and_says_so() {
    let chosen = pick(
        &catalogue(),
        NOW,
        &prefs(&[(key_for(Kind::Image), "openrouter/retired-draw")]),
        Mode::Manual,
        None,
        Some(Kind::Image),
    )
    .unwrap();
    assert_eq!(chosen.candidate.key(), "openrouter/new-draw");
    assert!(chosen.note.unwrap().contains("retired-draw"));
}

#[test]
fn the_automatic_sentinel_is_not_a_model_name() {
    assert_eq!(
        prefs(&[(key_for(Kind::Image), AUTOMATIC)]).default_for(Some(Kind::Image)),
        None
    );
}

/// The manifest says which keys this connector stores; the code fills in the menus behind them.
/// Two lists of the same keys is two places to forget one.
#[test]
fn the_manifest_declares_every_setting_the_code_writes() {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).expect("a manifest");
    let mut declared: Vec<&str> = manifest["user_config"]
        .as_object()
        .expect("user_config")
        .keys()
        .map(String::as_str)
        .collect();
    declared.sort_unstable();
    let mut built: Vec<String> = settings_form(&catalogue(), NOW)
        .into_iter()
        .map(|f| f.key)
        .collect();
    built.sort();
    assert_eq!(declared, built, "the manifest and the form disagree");
}

/// The menu is the catalogue, so the form cannot offer a model the tool would then refuse — and
/// it offers the older ones too, marked, because this is the one place choosing one on purpose
/// is possible.
#[test]
fn the_model_menus_are_built_from_the_models_there_are() {
    let form = settings_form(&catalogue(), NOW);
    let image = form
        .iter()
        .find(|f| f.key == key_for(Kind::Image))
        .expect("an image field");
    assert_eq!(image.options[0].value, AUTOMATIC);
    assert_eq!(
        image.options[0].detail.as_deref(),
        Some("today: openrouter/new-draw")
    );
    let offered: Vec<&str> = image.options[1..]
        .iter()
        .map(|o| o.value.as_str())
        .collect();
    assert_eq!(
        offered,
        [
            "openrouter/new-draw",
            "xai/new-draw",
            "openrouter/meta/muse-image",
            "openrouter/mid-draw",
            "openrouter/old-draw"
        ],
        "newest first, every picture model, and no speech model in the picture menu"
    );
    let old = image.options.last().unwrap();
    assert!(
        old.detail.as_deref().unwrap().contains("20 months old"),
        "{old:?}"
    );
    assert!(
        image.options[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("$30.00 / M drawn"),
        "a published rate is shown as the rate it is: {:?}",
        image.options[1]
    );
    assert!(
        image.options[3]
            .detail
            .as_deref()
            .unwrap()
            .contains("price not published"),
        "and most of them have none"
    );
}

#[test]
fn the_rule_is_a_menu_of_three() {
    let form = settings_form(&catalogue(), NOW);
    let rule = form
        .iter()
        .find(|f| f.key == DEFAULT_MODEL_RULE)
        .expect("the rule field");
    let values: Vec<&str> = rule.options.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, ["unnamed", "unattended", "always"]);
    assert_eq!(rule.default.as_deref(), Some("unnamed"));
}

// ---------------------------------------------------------------- what a call may ask for

#[test]
fn an_option_the_model_does_not_offer_is_refused_before_the_request() {
    let mut chosen = candidate("openrouter", "draw", Kind::Image, Some(30.0));
    chosen.info.capabilities.aspect_ratios = vec!["1:1".into(), "16:9".into()];

    let offered = MediaOptions {
        aspect_ratio: Some("16:9".into()),
        ..MediaOptions::default()
    };
    assert!(gantry_connector_media::check(&chosen, &offered).is_ok());

    let not_offered = MediaOptions {
        aspect_ratio: Some("21:9".into()),
        ..MediaOptions::default()
    };
    let err = gantry_connector_media::check(&chosen, &not_offered).unwrap_err();
    assert!(err.contains("21:9") && err.contains("16:9"), "{err}");
}

/// A model that lists nothing is asked without the option rather than refused: the list is the
/// provider's, and an empty one means "not published", not "none".
#[test]
fn a_model_that_lists_no_options_takes_whatever_it_is_given() {
    let chosen = candidate("openrouter", "draw", Kind::Image, Some(30.0));
    let options = MediaOptions {
        aspect_ratio: Some("21:9".into()),
        quality: Some("high".into()),
        ..MediaOptions::default()
    };
    assert!(gantry_connector_media::check(&chosen, &options).is_ok());
}

#[test]
fn a_duration_the_model_does_not_make_is_refused_with_the_ones_it_does() {
    let mut chosen = candidate("openrouter", "film", Kind::Video, Some(0.1));
    chosen.info.capabilities.durations = vec![4, 8];
    let options = MediaOptions {
        duration_seconds: Some(30),
        ..MediaOptions::default()
    };
    let err = gantry_connector_media::check(&chosen, &options).unwrap_err();
    assert!(err.contains("30-second") && err.contains("4, 8"), "{err}");
}

#[test]
fn the_kinds_are_named_the_way_the_arguments_spell_them() {
    assert_eq!(kind_name(Kind::Image), "image");
    assert_eq!(kind_name(Kind::Speech), "speech");
    assert_eq!(kind_name(Kind::Video), "video");
}

// ---------------------------------------------------------------- the list, and the card

/// The list is what stops the guessing, so the thing a call with no `model` would get is marked
/// in it. Without that the model has a list and no idea which of it is the answer.
#[test]
fn the_list_marks_what_a_call_with_no_model_would_use() {
    let (text, json) = listing(&catalogue(), NOW, None, "", 40);
    assert!(text.starts_with("5 models, newest first."), "{text}");
    let models = json["models"].as_array().unwrap();
    let defaults: Vec<&str> = models
        .iter()
        .filter(|m| m["default_without_a_model"] == serde_json::json!(true))
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(defaults, ["openrouter/new-draw", "openrouter/speak"]);
}

/// The old ones are not in the list, and their number is, so a model that remembers one is told
/// it exists rather than left to conclude it never did.
#[test]
fn the_list_leaves_out_what_the_chat_model_may_not_pick() {
    let (text, json) = listing(&catalogue(), NOW, Some(Kind::Image), "", 40);
    let ids: Vec<&str> = json["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&"openrouter/old-draw"), "{ids:?}");
    assert_eq!(json["hidden_as_too_old"], 1);
    assert!(text.contains("over a year old"), "{text}");
    assert!(text.contains("settings"), "{text}");
}

#[test]
fn a_kind_and_a_search_narrow_the_list() {
    let (_, json) = listing(&catalogue(), NOW, Some(Kind::Speech), "", 40);
    assert_eq!(json["matching"], 1);
    assert_eq!(json["models"][0]["id"], "openrouter/speak");

    let (_, json) = listing(&catalogue(), NOW, None, "xai new", 40);
    assert_eq!(json["matching"], 1);
    assert_eq!(json["models"][0]["id"], "xai/new-draw");
}

/// A limit that cuts the list says so, with the number it cut to, because a model that thinks it
/// has seen everything will conclude a model is missing rather than ask for more.
#[test]
fn a_cut_list_says_how_much_was_cut() {
    let (text, json) = listing(&catalogue(), NOW, None, "", 2);
    assert!(text.starts_with("2 of 5 models"), "{text}");
    assert!(text.contains("`limit`"), "{text}");
    assert_eq!(json["shown"], 2);
    assert_eq!(json["matching"], 5);
}

#[test]
fn a_search_that_matches_nothing_says_how_many_there_are() {
    let (text, json) = listing(&catalogue(), NOW, None, "sora", 40);
    assert!(text.contains("5 media models altogether"), "{text}");
    assert_eq!(json["shown"], 0);
}

/// The options in a row are the ones `check` compares against: one list, so the model cannot be
/// told one thing and refused by another. The price is a labelled string, because the number on
/// its own would be read as the price of one picture and it is a rate per million tokens of one.
#[test]
fn a_row_carries_the_options_the_model_would_be_refused_for() {
    let mut film = aged("openrouter", "film", Kind::Video, Some(0.12), 2);
    film.info.capabilities.durations = vec![4, 8];
    film.info.capabilities.resolutions = vec!["720p".into(), "1080p".into()];
    let (_, json) = listing(&[film], NOW, None, "", 40);
    let row = &json["models"][0];
    assert_eq!(row["durations_seconds"], serde_json::json!([4, 8]));
    assert_eq!(row["resolutions"], serde_json::json!(["720p", "1080p"]));
    assert!(
        row.get("voices").is_none(),
        "a list the provider never published is absent, not empty: {row}"
    );
    assert_eq!(row["price"], "$0.12 a second");
    assert!(row["released"].as_str().unwrap().contains("2026"), "{row}");
}

/// The card is where the money is agreed to, so it shows the model that is actually about to be
/// charged — resolved, not as the chat model typed it.
#[test]
fn the_card_offers_the_model_the_call_would_use() {
    let choice = model_choice(
        &catalogue(),
        NOW,
        &Preferences::default(),
        Mode::Manual,
        None,
        Some(Kind::Image),
    );
    assert_eq!(choice.key, "model");
    assert_eq!(choice.value.as_deref(), Some("openrouter/new-draw"));
    assert_eq!(choice.note, None);
    let offered: Vec<&str> = choice.options.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(
        offered,
        [
            "openrouter/new-draw",
            "xai/new-draw",
            "openrouter/meta/muse-image",
            "openrouter/mid-draw"
        ],
        "every picture model a chat model may pick, newest first, and nothing older than a year"
    );
    assert_eq!(
        choice.options[0].detail.as_deref(),
        Some("Aug 2026 · $30.00 / M drawn")
    );
}

/// A model the call named that is not on offer is **not** silently swapped: the card shows what
/// would run instead and says why, before anything is pressed.
#[test]
fn a_model_that_is_not_here_is_named_on_the_card_rather_than_swapped_quietly() {
    let choice = model_choice(
        &catalogue(),
        NOW,
        &Preferences::default(),
        Mode::Manual,
        Some("dall-e-9"),
        Some(Kind::Image),
    );
    assert_eq!(choice.value.as_deref(), Some("openrouter/new-draw"));
    let note = choice.note.expect("a reason");
    assert!(note.contains("dall-e-9"), "{note}");
}

#[test]
fn the_card_says_when_a_model_is_too_old_to_be_picked() {
    let choice = model_choice(
        &catalogue(),
        NOW,
        &Preferences::default(),
        Mode::Manual,
        Some("old-draw"),
        Some(Kind::Image),
    );
    let note = choice.note.expect("a reason");
    assert!(note.contains("20 months old"), "{note}");
    assert_eq!(choice.value.as_deref(), Some("openrouter/new-draw"));
}

/// The user's own older default is what runs, so it is on the card and in its menu — the one
/// place an older model appears in a list a chat model can see.
#[test]
fn an_older_default_is_on_the_card_it_will_run_from() {
    let choice = model_choice(
        &catalogue(),
        NOW,
        &prefs(&[(key_for(Kind::Image), "openrouter/old-draw")]),
        Mode::Manual,
        None,
        Some(Kind::Image),
    );
    assert_eq!(choice.value.as_deref(), Some("openrouter/old-draw"));
    assert!(
        choice
            .options
            .iter()
            .any(|o| o.value == "openrouter/old-draw"),
        "the value is always in the menu it is chosen from"
    );
}

#[test]
fn a_named_model_is_what_the_card_shows() {
    let choice = model_choice(
        &catalogue(),
        NOW,
        &Preferences::default(),
        Mode::Manual,
        Some("openrouter/mid-draw"),
        Some(Kind::Image),
    );
    assert_eq!(choice.value.as_deref(), Some("openrouter/mid-draw"));
    assert_eq!(choice.note, None);
}

/// With no kind and nothing usable there is still something to choose from: picking a model is
/// what says which kind the call was about.
#[test]
fn with_no_kind_at_all_the_menu_is_everything() {
    let choice = model_choice(
        &catalogue(),
        NOW,
        &Preferences::default(),
        Mode::Manual,
        None,
        None,
    );
    assert_eq!(choice.value, None);
    assert_eq!(choice.options.len(), 5, "every model a chat model may pick");
    assert!(
        choice
            .options
            .iter()
            .any(|o| o.detail.as_deref().is_some_and(|d| d.starts_with("speech"))),
        "a mixed menu says which kind each one is"
    );
}
