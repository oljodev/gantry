//! OpenRouter's `GET /key`: what the key is called and how much credit it has used.

use crate::provider::KeyInfo;

pub fn parse(json: &serde_json::Value) -> KeyInfo {
    let d = json.get("data").unwrap_or(json);
    KeyInfo {
        label: d.get("label").and_then(|v| v.as_str()).map(str::to_owned),
        limit_usd: d.get("limit").and_then(|v| v.as_f64()),
        limit_remaining_usd: d.get("limit_remaining").and_then(|v| v.as_f64()),
        usage_usd: d.get("usage").and_then(|v| v.as_f64()),
        is_free_tier: d.get("is_free_tier").and_then(|v| v.as_bool()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_documented_shape() {
        let json = serde_json::json!({ "data": {
            "label": "gantry-dev", "limit": 0.5, "usage": 0.031, "limit_remaining": 0.469, "is_free_tier": false
        }});
        let k = parse(&json);
        assert_eq!(k.label.as_deref(), Some("gantry-dev"));
        assert_eq!(k.limit_usd, Some(0.5));
        assert_eq!(k.usage_usd, Some(0.031));
        assert_eq!(k.is_free_tier, Some(false));
    }
}
