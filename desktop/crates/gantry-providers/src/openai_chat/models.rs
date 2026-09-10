//! `GET /models` → [`ModelInfo`]s. OpenRouter's list is rich; plain endpoints only have ids.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::profiles::ModelsParser;
use crate::{
    error::ProviderError,
    provider::{CacheSupport, Modality, ModelCapabilities, ModelInfo, Pricing, ReasoningSupport},
};

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: serde::Deserialize<'de>"))]
struct List<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct PlainModel {
    id: String,
    /// Unix seconds where the endpoint follows OpenAI's shape; absent on many that do not.
    created: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct XaiList {
    #[serde(default)]
    models: Vec<XaiModel>,
}

/// xAI prices are in hundred-thousandths of a US cent per token, i.e. `/ 10_000` gives
/// dollars per million tokens (grok-4: `30000` → $3).
#[derive(Debug, Deserialize)]
struct XaiModel {
    id: String,
    created: Option<i64>,
    #[serde(default)]
    input_modalities: Vec<String>,
    prompt_text_token_price: Option<f64>,
    cached_prompt_text_token_price: Option<f64>,
    completion_text_token_price: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OrModel {
    id: String,
    name: Option<String>,
    /// When OpenRouter first listed the model, in Unix seconds.
    created: Option<i64>,
    context_length: Option<u64>,
    pricing: Option<OrPricing>,
    top_provider: Option<OrTopProvider>,
    #[serde(default)]
    supported_parameters: Vec<String>,
    architecture: Option<OrArchitecture>,
    /// Named voices, on a text-to-speech model. Null on everything else.
    supported_voices: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OrPricing {
    prompt: Option<String>,
    completion: Option<String>,
    input_cache_read: Option<String>,
    /// Absolute dollars per unit, not per million: an image sent, an image produced, a call.
    image: Option<String>,
    image_output: Option<String>,
    request: Option<String>,
    /// Per token, like `prompt` and `completion`, and several times their rate.
    audio: Option<String>,
    audio_output: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrTopProvider {
    max_completion_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OrArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

/// One row of `GET /videos/models` or `GET /images/models`: the listings that say what a media
/// model lets you choose and what it charges. The plain list says neither — a video model's
/// token prices are all zero — so these are fetched beside it and merged in by id.
#[derive(Debug, Deserialize)]
struct MediaDetail {
    id: String,
    // Video: flat lists, and a price map whose keys name what is being counted.
    #[serde(default)]
    supported_aspect_ratios: Vec<String>,
    #[serde(default)]
    supported_resolutions: Vec<String>,
    #[serde(default)]
    supported_durations: Vec<u32>,
    #[serde(default)]
    pricing_skus: BTreeMap<String, String>,
    // Image: one map of parameters, each an enum of values or a numeric range.
    #[serde(default)]
    supported_parameters: BTreeMap<String, MediaParameter>,
}

#[derive(Debug, Deserialize)]
struct MediaParameter {
    #[serde(default)]
    values: Vec<String>,
}

/// Folds a media listing into models already parsed from the plain list. Anything the listing
/// mentions that the plain list never had is ignored: the plain rows carry the modalities the
/// router decides on, and a model without them would be unreachable anyway.
pub fn merge_media_details(list: &mut [ModelInfo], json: &serde_json::Value) {
    let Ok(details) = serde_json::from_value::<List<MediaDetail>>(json.clone()) else {
        return;
    };
    let by_id: std::collections::HashMap<String, MediaDetail> = details
        .data
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    for model in list.iter_mut() {
        let Some(detail) = by_id.get(&model.id) else {
            continue;
        };
        let param = |name: &str| {
            detail
                .supported_parameters
                .get(name)
                .map(|p| p.values.clone())
                .unwrap_or_default()
        };
        let caps = &mut model.capabilities;
        if !detail.supported_aspect_ratios.is_empty() {
            caps.aspect_ratios = detail.supported_aspect_ratios.clone();
        } else if !param("aspect_ratio").is_empty() {
            caps.aspect_ratios = param("aspect_ratio");
        }
        if !detail.supported_resolutions.is_empty() {
            caps.resolutions = detail.supported_resolutions.clone();
        } else if !param("resolution").is_empty() {
            caps.resolutions = param("resolution");
        }
        if !detail.supported_durations.is_empty() {
            caps.durations = detail.supported_durations.clone();
        }
        if !param("quality").is_empty() {
            caps.qualities = param("quality");
        }
        let per_second = seconds_pricing(&detail.pricing_skus);
        if !per_second.is_empty() {
            model
                .pricing
                .get_or_insert_with(|| Pricing {
                    input_per_mtok: 0.0,
                    output_per_mtok: 0.0,
                    cache_read_per_mtok: None,
                    image_input_usd: None,
                    image_output_usd: None,
                    request_usd: None,
                    audio_input_per_mtok: None,
                    audio_output_per_mtok: None,
                    video_per_second_usd: BTreeMap::new(),
                })
                .video_per_second_usd = per_second;
        }
    }
}

/// Dollars per second of video, by resolution, out of a price map with thirty-odd key shapes.
///
/// Only the keys that really are per-second are read: `duration_seconds…` in dollars and
/// `cents_per_second_output…` / `cents_per_video_output_second…` in cents. A model priced by the
/// token or by the megapixel-second cannot be turned into a per-second figure without knowing
/// what it will produce, and a made-up number is worse than an honest blank.
fn seconds_pricing(skus: &BTreeMap<String, String>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for (key, value) in skus {
        let Ok(price) = value.trim().parse::<f64>() else {
            continue;
        };
        let (rest, dollars) = if let Some(rest) = key.strip_prefix("duration_seconds") {
            (rest, price)
        } else if let Some(rest) = key.strip_prefix("cents_per_second_output") {
            (rest, price / 100.0)
        } else if let Some(rest) = key.strip_prefix("cents_per_video_output_second") {
            (rest, price / 100.0)
        } else {
            continue;
        };
        // What is left of the key is the resolution it applies to, where it names one:
        // `_480p`, `_with_audio_720p`. The bare key is the model's flat rate.
        let resolution = rest
            .rsplit('_')
            .find(|part| part.ends_with('p') || part.ends_with('K') || part.ends_with('k'))
            .unwrap_or_default()
            .to_owned();
        out.entry(resolution)
            .and_modify(|current: &mut f64| *current = current.min(dollars))
            .or_insert(dollars);
    }
    out
}

pub fn parse(
    parser: ModelsParser,
    json: &serde_json::Value,
) -> Result<Vec<ModelInfo>, ProviderError> {
    let bad =
        |e: serde_json::Error| ProviderError::interrupted(format!("malformed model list: {e}"));
    let mut models = match parser {
        ModelsParser::Plain => {
            let list: List<PlainModel> = serde_json::from_value(json.clone()).map_err(bad)?;
            list.data
                .into_iter()
                .map(|m| ModelInfo {
                    display_name: m.id.clone(),
                    created_at: m.created,
                    id: m.id,
                    context_window: None,
                    max_output: None,
                    pricing: None,
                    capabilities: ModelCapabilities::default(),
                })
                .collect::<Vec<_>>()
        }
        ModelsParser::OpenRouter => {
            let list: List<OrModel> = serde_json::from_value(json.clone()).map_err(bad)?;
            list.data.into_iter().map(from_openrouter).collect()
        }
        ModelsParser::XAi => {
            let list: XaiList = serde_json::from_value(json.clone()).map_err(bad)?;
            list.models.into_iter().map(from_xai).collect()
        }
    };
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

fn per_mtok(s: &Option<String>) -> Option<f64> {
    per_unit(s).map(|v| v * 1_000_000.0)
}

/// A price OpenRouter reports per unit rather than per token; zero means "not priced this way".
fn per_unit(s: &Option<String>) -> Option<f64> {
    let v = s.as_deref()?.trim().parse::<f64>().ok()?;
    (v > 0.0).then_some(v)
}

fn modality(name: &str) -> Option<Modality> {
    match name {
        "text" => Some(Modality::Text),
        "image" => Some(Modality::Image),
        "audio" => Some(Modality::Audio),
        "speech" => Some(Modality::Speech),
        "video" => Some(Modality::Video),
        "file" => Some(Modality::File),
        // A modality nobody has taught the app about is left out rather than guessed at: the
        // picker would otherwise file the model under a kind it cannot actually talk to.
        _ => None,
    }
}

fn from_xai(m: XaiModel) -> ModelInfo {
    let per_mtok = |p: Option<f64>| p.map(|v| v / 10_000.0);
    let pricing = match (
        per_mtok(m.prompt_text_token_price),
        per_mtok(m.completion_text_token_price),
    ) {
        (Some(input), Some(output)) => Some(Pricing {
            input_per_mtok: input,
            output_per_mtok: output,
            cache_read_per_mtok: per_mtok(m.cached_prompt_text_token_price),
            image_input_usd: None,
            image_output_usd: None,
            request_usd: None,
            audio_input_per_mtok: None,
            audio_output_per_mtok: None,
            video_per_second_usd: BTreeMap::new(),
        }),
        _ => None,
    };
    ModelInfo {
        display_name: m.id.clone(),
        created_at: m.created,
        context_window: None,
        max_output: None,
        pricing,
        capabilities: ModelCapabilities {
            input: {
                let mut input: Vec<Modality> = m
                    .input_modalities
                    .iter()
                    .filter_map(|x| modality(x))
                    .collect();
                if input.is_empty() {
                    input.push(Modality::Text);
                }
                input
            },
            output: vec![Modality::Text],
            vision: m.input_modalities.iter().any(|x| x == "image"),
            parallel_tools: true,
            streams_tool_args: true,
            prompt_caching: if m.cached_prompt_text_token_price.is_some() {
                CacheSupport::Automatic
            } else {
                CacheSupport::None
            },
            ..Default::default()
        },
        id: m.id,
    }
}

fn from_openrouter(m: OrModel) -> ModelInfo {
    let params = &m.supported_parameters;
    let has = |p: &str| params.iter().any(|x| x == p);
    let architecture = m.architecture.as_ref();
    let modalities = architecture
        .map(|a| a.input_modalities.clone())
        .unwrap_or_default();
    let input: Vec<Modality> = modalities.iter().filter_map(|x| modality(x)).collect();
    // Only OpenRouter's newer rows carry output modalities. An older row is a text model: that
    // is what every model on the list was when the field did not exist.
    let output: Vec<Modality> = architecture
        .map(|a| {
            a.output_modalities
                .iter()
                .filter_map(|x| modality(x))
                .collect::<Vec<_>>()
        })
        .filter(|o: &Vec<Modality>| !o.is_empty())
        .unwrap_or_else(|| vec![Modality::Text]);
    let pricing = m.pricing.as_ref().and_then(|p| {
        Some(Pricing {
            input_per_mtok: per_mtok(&p.prompt)?,
            output_per_mtok: per_mtok(&p.completion)?,
            cache_read_per_mtok: per_mtok(&p.input_cache_read),
            image_input_usd: per_unit(&p.image),
            image_output_usd: per_unit(&p.image_output),
            request_usd: per_unit(&p.request),
            audio_input_per_mtok: per_mtok(&p.audio),
            audio_output_per_mtok: per_mtok(&p.audio_output),
            video_per_second_usd: BTreeMap::new(),
        })
    });
    let capabilities = ModelCapabilities {
        input: if input.is_empty() {
            vec![Modality::Text]
        } else {
            input
        },
        output,
        tools: has("tools"),
        parallel_tools: has("parallel_tool_calls"),
        streams_tool_args: has("tools"),
        vision: modalities.iter().any(|x| x == "image"),
        pdf_input: modalities.iter().any(|x| x == "file"),
        reasoning: if has("reasoning") || has("reasoning_effort") {
            ReasoningSupport::Effort
        } else {
            ReasoningSupport::None
        },
        server_web_search: false,
        structured_output: has("structured_outputs") || has("response_format"),
        prompt_caching: if pricing
            .as_ref()
            .and_then(|p| p.cache_read_per_mtok)
            .is_some()
        {
            CacheSupport::Automatic
        } else {
            CacheSupport::None
        },
        voices: m.supported_voices.unwrap_or_default(),
        // The dedicated listings carry these; the chat-shaped rows do not (§5).
        aspect_ratios: Vec::new(),
        resolutions: Vec::new(),
        durations: Vec::new(),
        qualities: Vec::new(),
    };
    ModelInfo {
        display_name: m.name.clone().unwrap_or_else(|| m.id.clone()),
        created_at: m.created,
        id: m.id,
        context_window: m.context_length.and_then(|c| u32::try_from(c).ok()),
        max_output: m
            .top_provider
            .and_then(|t| t.max_completion_tokens)
            .and_then(|c| u32::try_from(c).ok()),
        pricing,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_listing_says_what_it_offers_and_what_a_second_costs() {
        let plain = serde_json::json!({ "data": [{
            "id": "alibaba/wan-3.0",
            "name": "Alibaba: Wan 3.0",
            "architecture": { "input_modalities": ["text", "image"], "output_modalities": ["video"] },
            "pricing": { "prompt": "0", "completion": "0" }
        }]});
        let mut models = parse(ModelsParser::OpenRouter, &plain).unwrap();
        merge_media_details(
            &mut models,
            &serde_json::json!({ "data": [{
                "id": "alibaba/wan-3.0",
                "supported_resolutions": ["480p", "720p", "1080p"],
                "supported_aspect_ratios": ["16:9", "9:16"],
                "supported_durations": [2, 3, 4],
                "pricing_skus": {
                    "duration_seconds_480p": "0.05",
                    "duration_seconds_720p": "0.1",
                    "duration_seconds_1080p": "0.2"
                }
            }]}),
        );
        let m = &models[0];
        assert_eq!(m.capabilities.resolutions, ["480p", "720p", "1080p"]);
        assert_eq!(m.capabilities.aspect_ratios, ["16:9", "9:16"]);
        assert_eq!(m.capabilities.durations, [2, 3, 4]);
        let per_second = &m.pricing.as_ref().unwrap().video_per_second_usd;
        assert_eq!(per_second.get("720p"), Some(&0.1));
        assert_eq!(per_second.get("1080p"), Some(&0.2));
    }

    #[test]
    fn an_image_listing_says_which_shapes_and_tiers_it_takes() {
        let plain = serde_json::json!({ "data": [{
            "id": "openai/gpt-image-2.5-flare",
            "architecture": { "input_modalities": ["text"], "output_modalities": ["image"] }
        }]});
        let mut models = parse(ModelsParser::OpenRouter, &plain).unwrap();
        merge_media_details(
            &mut models,
            &serde_json::json!({ "data": [{
                "id": "openai/gpt-image-2.5-flare",
                "supported_parameters": {
                    "aspect_ratio": { "type": "enum", "values": ["1:1", "16:9"] },
                    "quality": { "type": "enum", "values": ["low", "high"] },
                    "n": { "type": "range", "min": 1, "max": 10 }
                }
            }]}),
        );
        assert_eq!(models[0].capabilities.aspect_ratios, ["1:1", "16:9"]);
        assert_eq!(models[0].capabilities.qualities, ["low", "high"]);
    }

    #[test]
    fn a_price_that_is_not_by_the_second_is_left_blank_rather_than_invented() {
        // Cents where it says cents, and nothing at all for the models priced by the token or
        // by the megapixel-second: what those cost depends on what the model decides to make.
        let cents = seconds_pricing(&BTreeMap::from([
            ("cents_per_second_output_720p".to_owned(), "12.5".to_owned()),
            ("video_tokens".to_owned(), "0.0000035".to_owned()),
            (
                "cents_per_megapixel_second_precise".to_owned(),
                "7.5".to_owned(),
            ),
        ]));
        assert_eq!(cents.get("720p"), Some(&0.125));
        assert_eq!(cents.len(), 1, "only the keys that really are per second");
        assert!(
            seconds_pricing(&BTreeMap::from([(
                "video_tokens".to_owned(),
                "0.0000035".to_owned()
            )]))
            .is_empty()
        );
    }

    #[test]
    fn parses_an_openrouter_row() {
        let json = serde_json::json!({ "data": [{
            "id": "deepseek/deepseek-v4-flash",
            "name": "DeepSeek: DeepSeek V4 Flash 0423",
            "created": 1745366400,
            "context_length": 1048576,
            "architecture": { "input_modalities": ["text"] },
            "pricing": { "prompt": "0.000000088606", "completion": "0.000000177212" },
            "top_provider": { "max_completion_tokens": 32768 },
            "supported_parameters": ["max_tokens", "reasoning", "tools", "tool_choice", "structured_outputs"]
        }]});
        let models = parse(ModelsParser::OpenRouter, &json).unwrap();
        let m = &models[0];
        assert_eq!(m.display_name, "DeepSeek: DeepSeek V4 Flash 0423");
        assert_eq!(m.context_window, Some(1_048_576));
        assert_eq!(m.max_output, Some(32_768));
        let p = m.pricing.as_ref().unwrap();
        assert!((p.input_per_mtok - 0.088606).abs() < 1e-9);
        assert!(m.capabilities.tools);
        assert_eq!(m.capabilities.reasoning, ReasoningSupport::Effort);
        assert!(!m.capabilities.vision);
        assert!(m.capabilities.structured_output);
        assert_eq!(
            m.created_at,
            Some(1_745_366_400),
            "the release date the age filter reads"
        );
        assert_eq!(m.capabilities.input, vec![Modality::Text]);
        assert_eq!(m.capabilities.output, vec![Modality::Text]);
    }

    #[test]
    fn reads_the_modalities_and_the_per_image_prices_of_an_image_model() {
        let json = serde_json::json!({ "data": [{
            "id": "google/gemini-3-flash-image",
            "name": "Google: Gemini 3 Flash Image",
            "context_length": 32768,
            "architecture": {
                "input_modalities": ["text", "image"],
                "output_modalities": ["text", "image"]
            },
            "pricing": {
                "prompt": "0.0000003",
                "completion": "0.0000025",
                "image": "0.0001238",
                "image_output": "0.03",
                "request": "0"
            },
            "supported_parameters": ["max_tokens"]
        }]});
        let models = parse(ModelsParser::OpenRouter, &json).unwrap();
        let m = &models[0];
        assert_eq!(m.capabilities.input, vec![Modality::Text, Modality::Image]);
        assert_eq!(m.capabilities.output, vec![Modality::Text, Modality::Image]);
        assert!(m.capabilities.vision);
        let p = m.pricing.as_ref().unwrap();
        assert_eq!(p.image_input_usd, Some(0.0001238));
        assert_eq!(p.image_output_usd, Some(0.03));
        // A zero price is "not priced this way", not "free".
        assert_eq!(p.request_usd, None);
    }

    #[test]
    fn a_row_without_output_modalities_is_a_text_model() {
        let json = serde_json::json!({ "data": [{
            "id": "old/model",
            "architecture": { "input_modalities": ["text"] }
        }]});
        let models = parse(ModelsParser::OpenRouter, &json).unwrap();
        assert_eq!(models[0].capabilities.output, vec![Modality::Text]);
        assert_eq!(models[0].created_at, None, "a row without a date has none");
    }

    #[test]
    fn xai_prices_are_scaled_to_dollars_per_million() {
        let json = serde_json::json!({ "models": [{
            "id": "grok-4", "input_modalities": ["text", "image"], "output_modalities": ["text"],
            "prompt_text_token_price": 30000, "cached_prompt_text_token_price": 7500,
            "completion_text_token_price": 150000, "aliases": ["grok-4-latest"]
        }]});
        let models = parse(ModelsParser::XAi, &json).unwrap();
        let p = models[0].pricing.as_ref().unwrap();
        assert!((p.input_per_mtok - 3.0).abs() < 1e-9);
        assert!((p.output_per_mtok - 15.0).abs() < 1e-9);
        assert!((p.cache_read_per_mtok.unwrap() - 0.75).abs() < 1e-9);
        assert!(models[0].capabilities.vision);
    }

    #[test]
    fn plain_lists_only_carry_ids() {
        let json = serde_json::json!({ "data": [{ "id": "grok-4" }, { "id": "grok-3" }] });
        let models = parse(ModelsParser::Plain, &json).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["grok-3", "grok-4"]
        );
        assert!(models[0].pricing.is_none());
    }
}
