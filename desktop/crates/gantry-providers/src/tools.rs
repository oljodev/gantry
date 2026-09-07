//! Tool naming and schema sanitizing shared by every client (docs/plan/02 §3, "shared rules").
//!
//! Model-facing names are `<connector>__<tool>` inside `^[a-zA-Z][a-zA-Z0-9_-]{0,63}$`; a name
//! that would overflow is cut and given a six-character hash suffix, and the caller keeps a
//! [`ToolNameMap`] to translate back. Schemas are cleaned per provider dialect before they are
//! sent: `$ref`/`$defs` inlined, rejected keywords dropped, `type: object` guaranteed at the
//! root.

use std::collections::HashMap;

use gantry_core::ProviderKind;
use serde_json::{Map, Value, json};

pub const NAME_MAX: usize = 64;
pub const SEPARATOR: &str = "__";

/// The model-facing name for `tool` of `connector`, always valid for every provider.
#[must_use]
pub fn model_tool_name(connector: &str, tool: &str) -> String {
    let raw = format!("{}{SEPARATOR}{}", clean(connector), clean(tool));
    if raw.len() <= NAME_MAX {
        return raw;
    }
    let hash = short_hash(&raw);
    let keep = NAME_MAX - hash.len() - 1;
    let mut cut: String = raw.chars().take(keep).collect();
    cut.push('_');
    cut.push_str(&hash);
    cut
}

/// Replaces characters outside the allowed set and makes sure the name starts with a letter.
fn clean(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !out.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        out.insert(0, 't');
    }
    out
}

fn short_hash(s: &str) -> String {
    // FNV-1a: stable, dependency-free, six hex chars is plenty for a per-request map.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:06x}", h & 0xff_ffff)
}

/// Model-facing name → `(connector, tool)`, built per request.
#[derive(Debug, Default, Clone)]
pub struct ToolNameMap {
    names: HashMap<String, (String, String)>,
}

impl ToolNameMap {
    /// Registers a tool and returns its model-facing name.
    pub fn insert(&mut self, connector: &str, tool: &str) -> String {
        let name = model_tool_name(connector, tool);
        self.names
            .insert(name.clone(), (connector.to_owned(), tool.to_owned()));
        name
    }

    #[must_use]
    pub fn resolve(&self, model_name: &str) -> Option<(&str, &str)> {
        self.names
            .get(model_name)
            .map(|(c, t)| (c.as_str(), t.as_str()))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Cleans JSON schemas for one provider dialect.
#[derive(Debug, Clone, Copy)]
pub struct ToolSchemaSanitizer {
    provider: ProviderKind,
}

/// Keywords no dialect needs and some reject.
const DROP_ALWAYS: &[&str] = &["$schema", "$id", "$comment", "$defs", "definitions"];
/// Gemini's OpenAPI subset does not know these.
const DROP_GEMINI: &[&str] = &[
    "additionalProperties",
    "patternProperties",
    "examples",
    "const",
    "default",
    "$ref",
];

impl ToolSchemaSanitizer {
    #[must_use]
    pub fn for_provider(provider: ProviderKind) -> Self {
        Self { provider }
    }

    /// A copy of `schema` the provider accepts. `strict` additionally closes every object
    /// (`additionalProperties: false`, all properties required), the OpenAI strict contract.
    #[must_use]
    pub fn sanitize(&self, schema: &Value, strict: bool) -> Value {
        let defs = collect_defs(schema);
        let mut out = self.walk(schema, &defs, 0);
        let root = match out.as_object_mut() {
            Some(o) => o,
            None => {
                out = json!({});
                out.as_object_mut().expect("object")
            }
        };
        root.insert("type".into(), json!("object"));
        root.entry("properties").or_insert_with(|| json!({}));
        if strict {
            close_objects(&mut out);
        }
        out
    }

    fn walk(&self, v: &Value, defs: &Map<String, Value>, depth: usize) -> Value {
        match v {
            Value::Object(o) => {
                if let Some(Value::String(r)) = o.get("$ref")
                    && depth < 16
                    && let Some(target) = r.rsplit('/').next().and_then(|name| defs.get(name))
                {
                    return self.walk(target, defs, depth + 1);
                }
                let mut out = Map::new();
                for (k, val) in o {
                    if DROP_ALWAYS.contains(&k.as_str()) {
                        continue;
                    }
                    if self.provider == ProviderKind::Gemini && DROP_GEMINI.contains(&k.as_str()) {
                        continue;
                    }
                    out.insert(k.clone(), self.walk(val, defs, depth + 1));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|i| self.walk(i, defs, depth + 1))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

fn collect_defs(schema: &Value) -> Map<String, Value> {
    let mut defs = Map::new();
    for key in ["$defs", "definitions"] {
        if let Some(Value::Object(d)) = schema.get(key) {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }
    }
    defs
}

fn close_objects(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("object") {
                let keys: Vec<Value> = o
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|p| p.keys().map(|k| json!(k)).collect())
                    .unwrap_or_default();
                o.insert("required".into(), Value::Array(keys));
                o.insert("additionalProperties".into(), json!(false));
            }
            for val in o.values_mut() {
                close_objects(val);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(close_objects),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_namespaced_and_bounded() {
        assert_eq!(
            model_tool_name("filesystem", "read_file"),
            "filesystem__read_file"
        );
        assert_eq!(model_tool_name("9 bad id", "x.y"), "t9_bad_id__x_y");
        let long = model_tool_name("a-very-long-connector-identifier", &"t".repeat(80));
        assert_eq!(long.len(), NAME_MAX);
        let mut map = ToolNameMap::default();
        let n = map.insert("a-very-long-connector-identifier", &"t".repeat(80));
        assert_eq!(n, long);
        assert_eq!(
            map.resolve(&n).map(|(c, t)| (c.to_owned(), t.len())),
            Some(("a-very-long-connector-identifier".to_owned(), 80))
        );
        assert!(map.resolve("nope").is_none());
    }

    #[test]
    fn refs_are_inlined_and_the_root_is_an_object() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": { "Path": { "type": "string", "description": "a path" } },
            "type": "object",
            "properties": { "path": { "$ref": "#/$defs/Path" }, "n": { "type": "integer", "default": 1 } }
        });
        let s =
            ToolSchemaSanitizer::for_provider(ProviderKind::OpenAiChat).sanitize(&schema, false);
        assert!(s.get("$defs").is_none() && s.get("$schema").is_none());
        assert_eq!(s["properties"]["path"]["type"], "string");
        assert_eq!(s["properties"]["n"]["default"], 1);
        let g = ToolSchemaSanitizer::for_provider(ProviderKind::Gemini).sanitize(&schema, false);
        assert!(g["properties"]["n"].get("default").is_none());
        let empty = ToolSchemaSanitizer::for_provider(ProviderKind::Anthropic)
            .sanitize(&json!(true), false);
        assert_eq!(empty, json!({ "type": "object", "properties": {} }));
    }

    #[test]
    fn strict_closes_every_object() {
        let schema = json!({ "type": "object", "properties": { "a": { "type": "string" }, "b": { "type": "object", "properties": { "c": {} } } } });
        let s = ToolSchemaSanitizer::for_provider(ProviderKind::OpenAiResponses)
            .sanitize(&schema, true);
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["required"], json!(["a", "b"]));
        assert_eq!(s["properties"]["b"]["required"], json!(["c"]));
    }
}
