//! `GET /models` → [`ModelInfo`]s. OpenRouter's list is rich; plain endpoints only have ids.

use serde::Deserialize;

use super::profiles::ModelsParser;
use crate::{
    error::ProviderError,
    provider::{CacheSupport, ModelCapabilities, ModelInfo, Pricing, ReasoningSupport},
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
}

#[derive(Debug, Deserialize)]
struct OrModel {
    id: String,
    name: Option<String>,
    context_length: Option<u64>,
    pricing: Option<OrPricing>,
    top_provider: Option<OrTopProvider>,
    #[serde(default)]
    supported_parameters: Vec<String>,
    architecture: Option<OrArchitecture>,
}

#[derive(Debug, Deserialize)]
struct OrPricing {
    prompt: Option<String>,
    completion: Option<String>,
    input_cache_read: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrTopProvider {
    max_completion_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OrArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
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
    };
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

fn per_mtok(s: &Option<String>) -> Option<f64> {
    s.as_deref()?
        .trim()
        .parse::<f64>()
        .ok()
        .map(|v| v * 1_000_000.0)
}

fn from_openrouter(m: OrModel) -> ModelInfo {
    let params = &m.supported_parameters;
    let has = |p: &str| params.iter().any(|x| x == p);
    let modalities = m
        .architecture
        .as_ref()
        .map(|a| a.input_modalities.clone())
        .unwrap_or_default();
    let pricing = m.pricing.as_ref().and_then(|p| {
        Some(Pricing {
            input_per_mtok: per_mtok(&p.prompt)?,
            output_per_mtok: per_mtok(&p.completion)?,
            cache_read_per_mtok: per_mtok(&p.input_cache_read),
        })
    });
    let capabilities = ModelCapabilities {
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
        prompt_caching: if pricing.and_then(|p| p.cache_read_per_mtok).is_some() {
            CacheSupport::Automatic
        } else {
            CacheSupport::None
        },
    };
    ModelInfo {
        display_name: m.name.clone().unwrap_or_else(|| m.id.clone()),
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
    fn parses_an_openrouter_row() {
        let json = serde_json::json!({ "data": [{
            "id": "deepseek/deepseek-v4-flash",
            "name": "DeepSeek: DeepSeek V4 Flash 0423",
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
        let p = m.pricing.unwrap();
        assert!((p.input_per_mtok - 0.088606).abs() < 1e-9);
        assert!(m.capabilities.tools);
        assert_eq!(m.capabilities.reasoning, ReasoningSupport::Effort);
        assert!(!m.capabilities.vision);
        assert!(m.capabilities.structured_output);
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
