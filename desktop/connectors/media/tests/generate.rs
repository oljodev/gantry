//! Choosing the model, and refusing before the money is spent.
//!
//! The endpoints themselves are M4's and are tested in `gantry-providers`; what is new here is
//! everything that happens before one is called. None of it needs a network, which is the point:
//! every refusal this connector can make, it makes for free.

use std::collections::BTreeMap;

use gantry_connector_media::{
    AUTOMATIC, Candidate, Kind, MANIFEST, Preferences, choose, definitions, key_for, kind_name,
    listing, parse_kind, rank, settings_form,
};
use gantry_core::{MediaOptions, ProviderId, RiskTier};
use gantry_providers::{ModelCapabilities, ModelInfo, Pricing, provider::Modality};

fn model(id: &str, out: Modality, price: Option<f64>) -> ModelInfo {
    let capabilities = ModelCapabilities {
        output: vec![out],
        ..ModelCapabilities::default()
    };
    ModelInfo {
        id: id.to_owned(),
        display_name: id.to_owned(),
        created_at: None,
        context_window: None,
        max_output: None,
        // Each kind is priced where its own endpoint publishes it (02 §4b): an image by the
        // picture, speech by the token, video by the second. A helper that put every price in
        // one field would make `unit_cost` look like it worked when it did not.
        pricing: price.map(|p| Pricing {
            input_per_mtok: 0.0,
            output_per_mtok: if out == Modality::Speech { p } else { 0.0 },
            cache_read_per_mtok: None,
            image_input_usd: None,
            image_output_usd: (out == Modality::Image).then_some(p),
            request_usd: None,
            audio_input_per_mtok: None,
            audio_output_per_mtok: None,
            video_per_second_usd: if out == Modality::Video {
                std::collections::BTreeMap::from([("720p".to_owned(), p)])
            } else {
                std::collections::BTreeMap::new()
            },
        }),
        capabilities,
    }
}

fn candidate(provider: &str, id: &str, kind: Kind, price: Option<f64>) -> Candidate {
    let out = match kind {
        Kind::Image => Modality::Image,
        Kind::Speech => Modality::Speech,
        Kind::Video => Modality::Video,
    };
    Candidate {
        provider: ProviderId::new(provider.to_owned()),
        model: id.to_owned(),
        kind,
        info: model(id, out, price),
    }
}

fn catalogue() -> Vec<Candidate> {
    vec![
        candidate("openrouter", "cheap-draw", Kind::Image, Some(0.01)),
        candidate("openrouter", "dear-draw", Kind::Image, Some(0.40)),
        candidate("openrouter", "mystery-draw", Kind::Image, None),
        candidate("openrouter", "speak", Kind::Speech, Some(0.02)),
        candidate("xai", "cheap-draw", Kind::Image, Some(0.02)),
    ]
}

#[test]
fn the_one_tool_is_an_external_write_and_never_runs_beside_itself() {
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

#[test]
fn without_a_model_the_cheapest_of_that_kind_answers() {
    let chosen = choose(&catalogue(), None, Some(Kind::Image), None).unwrap();
    assert_eq!(chosen.key(), "openrouter/cheap-draw");
    let chosen = choose(&catalogue(), None, Some(Kind::Speech), None).unwrap();
    assert_eq!(chosen.key(), "openrouter/speak");
}

/// The order is the default, so this is the test that says what the default is. An unknown
/// price is the one that cannot be defended afterwards, so it sorts last however early its name
/// would have put it.
#[test]
fn the_order_is_cheapest_first_and_unpriced_last() {
    let mut models = vec![
        candidate("openrouter", "aaa-draw", Kind::Image, None),
        candidate("openrouter", "zzz-draw", Kind::Image, Some(1.0)),
        candidate("openrouter", "mmm-draw", Kind::Image, Some(0.5)),
        candidate("openrouter", "bbb-draw", Kind::Image, Some(0.5)),
    ];
    rank(&mut models);
    let order: Vec<String> = models.iter().map(|c| c.model.clone()).collect();
    assert_eq!(order, ["bbb-draw", "mmm-draw", "zzz-draw", "aaa-draw"]);
    assert_eq!(
        choose(&models, None, Some(Kind::Image), None)
            .unwrap()
            .model,
        "bbb-draw",
        "the cheapest is what a call with no model gets"
    );
}

#[test]
fn a_bare_model_id_works_when_only_one_provider_has_it() {
    let chosen = choose(&catalogue(), Some("dear-draw"), None, None).unwrap();
    assert_eq!(chosen.key(), "openrouter/dear-draw");
}

#[test]
fn a_bare_id_on_two_providers_asks_which() {
    let err = choose(&catalogue(), Some("cheap-draw"), None, None).unwrap_err();
    assert!(err.contains("openrouter/cheap-draw"), "{err}");
    assert!(err.contains("xai/cheap-draw"), "{err}");
}

#[test]
fn a_full_key_beats_the_ambiguity() {
    let chosen = choose(&catalogue(), Some("xai/cheap-draw"), None, None).unwrap();
    assert_eq!(chosen.provider.as_str(), "xai");
}

/// The error is the next call's instructions: a model that named something that is not there
/// gets real ids back rather than "not found".
#[test]
fn an_unknown_model_is_refused_with_what_there_is() {
    let err = choose(&catalogue(), Some("dall-e-9"), Some(Kind::Image), None).unwrap_err();
    assert!(err.contains("dall-e-9"), "{err}");
    assert!(err.contains("openrouter/cheap-draw"), "{err}");
    assert!(
        !err.contains("openrouter/speak"),
        "a picture question is not answered with a voice model: {err}"
    );
}

#[test]
fn a_model_that_makes_the_wrong_thing_is_refused_before_it_is_paid_for() {
    let err = choose(
        &catalogue(),
        Some("openrouter/speak"),
        Some(Kind::Image),
        None,
    )
    .unwrap_err();
    assert!(
        err.contains("spoken audio") && err.contains("a picture"),
        "{err}"
    );
}

#[test]
fn no_model_and_no_kind_is_a_question_rather_than_a_guess() {
    let err = choose(&catalogue(), None, None, None).unwrap_err();
    assert!(err.contains("`kind`"), "{err}");
}

#[test]
fn a_kind_nobody_can_make_says_so() {
    let err = choose(&catalogue(), None, Some(Kind::Video), None).unwrap_err();
    assert!(err.contains("a video clip"), "{err}");
    assert!(err.contains("Available:"), "{err}");
}

#[test]
fn with_nothing_at_all_the_error_names_the_setting_that_fixes_it() {
    let err = choose(&[], None, Some(Kind::Image), None).unwrap_err();
    assert!(err.contains("Settings"), "{err}");
}

#[test]
fn an_option_the_model_does_not_offer_is_refused_before_the_request() {
    let mut chosen = candidate("openrouter", "draw", Kind::Image, Some(0.01));
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
    let chosen = candidate("openrouter", "draw", Kind::Image, Some(0.01));
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

/// The list is what stops the guessing, so the thing a call with no `model` would get is marked
/// in it. Without that the model has a list and no idea which of it is the answer.
#[test]
fn the_list_marks_what_a_call_with_no_model_would_use() {
    let (text, json) = listing(&catalogue(), None, "", 40);
    assert!(text.starts_with("5 models, cheapest first."), "{text}");
    let models = json["models"].as_array().unwrap();
    let defaults: Vec<&str> = models
        .iter()
        .filter(|m| m["default_without_a_model"] == serde_json::json!(true))
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(defaults, ["openrouter/cheap-draw", "openrouter/speak"]);
    assert_eq!(json["available"], 5);
}

#[test]
fn a_kind_and_a_search_narrow_the_list() {
    let (_, json) = listing(&catalogue(), Some(Kind::Speech), "", 40);
    assert_eq!(json["matching"], 1);
    assert_eq!(json["models"][0]["id"], "openrouter/speak");

    let (_, json) = listing(&catalogue(), None, "xai cheap", 40);
    assert_eq!(json["matching"], 1);
    assert_eq!(json["models"][0]["id"], "xai/cheap-draw");
}

/// A limit that cuts the list says so, with the number it cut to, because a model that thinks it
/// has seen everything will conclude a model is missing rather than ask for more.
#[test]
fn a_cut_list_says_how_much_was_cut() {
    let (text, json) = listing(&catalogue(), None, "", 2);
    assert!(text.starts_with("2 of 5 models"), "{text}");
    assert!(text.contains("`limit`"), "{text}");
    assert_eq!(json["shown"], 2);
    assert_eq!(json["matching"], 5);
}

#[test]
fn a_search_that_matches_nothing_says_how_many_there_are() {
    let (text, json) = listing(&catalogue(), None, "sora", 40);
    assert!(text.contains("5 media models altogether"), "{text}");
    assert_eq!(json["shown"], 0);
}

/// The options in a row are the ones `check` compares against: one list, so the model cannot be
/// told one thing and refused by another.
#[test]
fn a_row_carries_the_options_the_model_would_be_refused_for() {
    let mut film = candidate("openrouter", "film", Kind::Video, Some(0.1));
    film.info.capabilities.durations = vec![4, 8];
    film.info.capabilities.resolutions = vec!["720p".into(), "1080p".into()];
    let (_, json) = listing(&[film], None, "", 40);
    let row = &json["models"][0];
    assert_eq!(row["durations_seconds"], serde_json::json!([4, 8]));
    assert_eq!(row["resolutions"], serde_json::json!(["720p", "1080p"]));
    assert!(
        row.get("voices").is_none(),
        "a list the provider never published is absent, not empty: {row}"
    );
    assert_eq!(row["unit_cost_usd"], serde_json::json!(0.1));
}

/// The manifest says which keys this connector stores; the code fills in the menus behind them.
/// Two lists of the same keys is two places to forget one, so they are compared here rather than
/// hoped about.
#[test]
fn the_manifest_declares_every_setting_the_code_writes() {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).expect("a manifest");
    let declared: Vec<&str> = manifest["user_config"]
        .as_object()
        .expect("user_config")
        .keys()
        .map(String::as_str)
        .collect();
    let mut built: Vec<String> = settings_form(&catalogue())
        .into_iter()
        .map(|f| f.key)
        .collect();
    built.sort();
    assert_eq!(declared, built, "the manifest and the form disagree");
}

/// The menu is the catalogue, so the form cannot offer a model the tool would then refuse — and
/// "cheapest available" is a real entry rather than an empty box, because a blank first row
/// reads as a question nobody has answered.
#[test]
fn the_model_menus_are_built_from_the_models_there_are() {
    let mut available = catalogue();
    rank(&mut available);
    let form = settings_form(&available);
    let image = form
        .iter()
        .find(|f| f.key == key_for(Kind::Image))
        .expect("an image field");
    assert_eq!(image.options[0].value, AUTOMATIC);
    assert_eq!(
        image.options[0].detail.as_deref(),
        Some("today: openrouter/cheap-draw")
    );
    let offered: Vec<&str> = image.options[1..]
        .iter()
        .map(|o| o.value.as_str())
        .collect();
    assert_eq!(
        offered,
        [
            "openrouter/cheap-draw",
            "xai/cheap-draw",
            "openrouter/dear-draw",
            "openrouter/mystery-draw"
        ],
        "cheapest first, unpriced last, and no speech model in the picture menu"
    );
    assert_eq!(image.options[1].detail.as_deref(), Some("$0.0100 a unit"));
}

#[test]
fn a_default_model_answers_a_call_that_names_none() {
    let prefs = Preferences::read(&BTreeMap::from([(
        key_for(Kind::Image).to_owned(),
        "openrouter/dear-draw".to_owned(),
    )]));
    let preferred = prefs.default_for(Some(Kind::Image));
    let chosen = choose(&catalogue(), None, Some(Kind::Image), preferred).unwrap();
    assert_eq!(chosen.key(), "openrouter/dear-draw");
    assert_eq!(
        prefs.default_for(Some(Kind::Speech)),
        None,
        "a kind with no default of its own still takes the cheapest"
    );
}

/// A model the call named is never replaced by a setting: the setting is what happens when
/// nobody said anything.
#[test]
fn a_named_model_beats_the_users_default() {
    let prefs = Preferences::read(&BTreeMap::from([(
        key_for(Kind::Image).to_owned(),
        "openrouter/dear-draw".to_owned(),
    )]));
    let chosen = choose(
        &catalogue(),
        Some("openrouter/cheap-draw"),
        Some(Kind::Image),
        prefs.default_for(Some(Kind::Image)),
    )
    .unwrap();
    assert_eq!(chosen.key(), "openrouter/cheap-draw");
}

/// A provider key removed, or a model retired: the preference stops being available and the
/// cheapest answers rather than the call failing over a setting.
#[test]
fn a_default_that_is_gone_falls_through_to_the_cheapest() {
    let chosen = choose(
        &catalogue(),
        None,
        Some(Kind::Image),
        Some("openrouter/retired-draw"),
    )
    .unwrap();
    assert_eq!(chosen.key(), "openrouter/cheap-draw");
}

#[test]
fn the_automatic_sentinel_is_not_a_model_name() {
    let prefs = Preferences::read(&BTreeMap::from([(
        key_for(Kind::Image).to_owned(),
        AUTOMATIC.to_owned(),
    )]));
    assert_eq!(prefs.default_for(Some(Kind::Image)), None);
}

/// The ceiling is checked here, against the published price, before anything is sent — the
/// whole point being that a refusal costs nothing and a charge cannot be taken back.
#[test]
fn a_ceiling_refuses_the_expensive_one_before_the_request() {
    let prefs = Preferences::read(&BTreeMap::from([(
        gantry_connector_media::MAX_COST_USD.to_owned(),
        "$0.10".to_owned(),
    )]));
    assert_eq!(prefs.ceiling, Some(0.10));
    let cheap = candidate("openrouter", "cheap-draw", Kind::Image, Some(0.01));
    assert!(prefs.affordable(&cheap).is_ok());

    let dear = candidate("openrouter", "dear-draw", Kind::Image, Some(0.40));
    let err = prefs.affordable(&dear).unwrap_err();
    assert!(err.contains("$0.4000") && err.contains("$0.10"), "{err}");
    assert!(err.contains("list_models"), "{err}");
}

/// A price nobody published cannot be shown to be under a ceiling. Same reasoning as the
/// automatic choice: the unknown one is the one that cannot be defended afterwards.
#[test]
fn a_model_with_no_price_is_refused_while_a_ceiling_stands() {
    let prefs = Preferences::read(&BTreeMap::from([(
        gantry_connector_media::MAX_COST_USD.to_owned(),
        "1".to_owned(),
    )]));
    let mystery = candidate("openrouter", "mystery-draw", Kind::Image, None);
    let err = prefs.affordable(&mystery).unwrap_err();
    assert!(err.contains("publishes no price"), "{err}");

    let none = Preferences::default();
    assert!(
        none.affordable(&mystery).is_ok(),
        "with no ceiling set, an unpriced model is only ever a model nobody chose automatically"
    );
}

/// Settings are typed by people. A ceiling that cannot be read is not a reason to stop drawing.
#[test]
fn an_unreadable_ceiling_is_no_ceiling() {
    for text in ["", "  ", "lots", "-3", "0"] {
        let prefs = Preferences::read(&BTreeMap::from([(
            gantry_connector_media::MAX_COST_USD.to_owned(),
            text.to_owned(),
        )]));
        assert_eq!(prefs.ceiling, None, "{text:?}");
    }
}
