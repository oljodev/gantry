//! Choosing the model, and refusing before the money is spent.
//!
//! The endpoints themselves are M4's and are tested in `gantry-providers`; what is new here is
//! everything that happens before one is called. None of it needs a network, which is the point:
//! every refusal this connector can make, it makes for free.

use gantry_connector_media::{Candidate, Kind, choose, definitions, kind_name, parse_kind, rank};
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
        pricing: price.map(|p| Pricing {
            input_per_mtok: 0.0,
            output_per_mtok: 0.0,
            cache_read_per_mtok: None,
            image_input_usd: None,
            image_output_usd: Some(p),
            request_usd: None,
            audio_input_per_mtok: None,
            audio_output_per_mtok: None,
            video_per_second_usd: std::collections::BTreeMap::new(),
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
    assert_eq!(defs.len(), 1, "one connector, one tool");
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
    let chosen = choose(&catalogue(), None, Some(Kind::Image)).unwrap();
    assert_eq!(chosen.key(), "openrouter/cheap-draw");
    let chosen = choose(&catalogue(), None, Some(Kind::Speech)).unwrap();
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
        choose(&models, None, Some(Kind::Image)).unwrap().model,
        "bbb-draw",
        "the cheapest is what a call with no model gets"
    );
}

#[test]
fn a_bare_model_id_works_when_only_one_provider_has_it() {
    let chosen = choose(&catalogue(), Some("dear-draw"), None).unwrap();
    assert_eq!(chosen.key(), "openrouter/dear-draw");
}

#[test]
fn a_bare_id_on_two_providers_asks_which() {
    let err = choose(&catalogue(), Some("cheap-draw"), None).unwrap_err();
    assert!(err.contains("openrouter/cheap-draw"), "{err}");
    assert!(err.contains("xai/cheap-draw"), "{err}");
}

#[test]
fn a_full_key_beats_the_ambiguity() {
    let chosen = choose(&catalogue(), Some("xai/cheap-draw"), None).unwrap();
    assert_eq!(chosen.provider.as_str(), "xai");
}

/// The error is the next call's instructions: a model that named something that is not there
/// gets real ids back rather than "not found".
#[test]
fn an_unknown_model_is_refused_with_what_there_is() {
    let err = choose(&catalogue(), Some("dall-e-9"), Some(Kind::Image)).unwrap_err();
    assert!(err.contains("dall-e-9"), "{err}");
    assert!(err.contains("openrouter/cheap-draw"), "{err}");
    assert!(
        !err.contains("openrouter/speak"),
        "a picture question is not answered with a voice model: {err}"
    );
}

#[test]
fn a_model_that_makes_the_wrong_thing_is_refused_before_it_is_paid_for() {
    let err = choose(&catalogue(), Some("openrouter/speak"), Some(Kind::Image)).unwrap_err();
    assert!(
        err.contains("spoken audio") && err.contains("a picture"),
        "{err}"
    );
}

#[test]
fn no_model_and_no_kind_is_a_question_rather_than_a_guess() {
    let err = choose(&catalogue(), None, None).unwrap_err();
    assert!(err.contains("`kind`"), "{err}");
}

#[test]
fn a_kind_nobody_can_make_says_so() {
    let err = choose(&catalogue(), None, Some(Kind::Video)).unwrap_err();
    assert!(err.contains("a video clip"), "{err}");
    assert!(err.contains("Available:"), "{err}");
}

#[test]
fn with_nothing_at_all_the_error_names_the_setting_that_fixes_it() {
    let err = choose(&[], None, Some(Kind::Image)).unwrap_err();
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
