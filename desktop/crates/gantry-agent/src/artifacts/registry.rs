//! The renderer registry (docs/plan/13 §3): the types the model may create, what renders
//! them and how they are saved. The tool schema's `type` enum is generated from this list and
//! the frontend mirrors it in `features/artifacts/registry.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    /// The app renders it itself.
    None,
    /// Rendered inside the sandboxed iframe; the tool result waits for its report.
    Sandbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub content_kind: ContentKind,
    pub execution: Execution,
    /// Download extensions, the first one default; `code` derives its own from `language`.
    pub extensions: &'static [&'static str],
    pub editable: bool,
    /// Renders progressively while the arguments stream.
    pub streams_render: bool,
    pub mime: &'static str,
}

pub const TYPES: &[TypeInfo] = &[
    TypeInfo {
        id: "markdown",
        label: "Markdown",
        content_kind: ContentKind::Text,
        execution: Execution::None,
        extensions: &["md"],
        editable: true,
        streams_render: true,
        mime: "text/markdown",
    },
    TypeInfo {
        id: "code",
        label: "Code",
        content_kind: ContentKind::Text,
        execution: Execution::None,
        extensions: &["txt"],
        editable: true,
        streams_render: true,
        mime: "text/plain",
    },
    TypeInfo {
        id: "svg",
        label: "SVG",
        content_kind: ContentKind::Text,
        execution: Execution::None,
        extensions: &["svg"],
        editable: true,
        streams_render: false,
        mime: "image/svg+xml",
    },
    TypeInfo {
        id: "html",
        label: "HTML",
        content_kind: ContentKind::Text,
        execution: Execution::Sandbox,
        extensions: &["html"],
        editable: true,
        streams_render: false,
        mime: "text/html",
    },
    TypeInfo {
        id: "mermaid",
        label: "Mermaid",
        content_kind: ContentKind::Text,
        execution: Execution::Sandbox,
        extensions: &["mmd"],
        editable: true,
        streams_render: false,
        mime: "text/plain",
    },
    TypeInfo {
        id: "react",
        label: "React",
        content_kind: ContentKind::Text,
        execution: Execution::Sandbox,
        extensions: &["tsx", "jsx"],
        editable: true,
        streams_render: false,
        mime: "text/plain",
    },
];

#[must_use]
pub fn get(id: &str) -> Option<&'static TypeInfo> {
    TYPES.iter().find(|t| t.id == id)
}

#[must_use]
pub fn ids() -> Vec<&'static str> {
    TYPES.iter().map(|t| t.id).collect()
}

/// A few well-known languages → file extension for `code` artifacts; unknown ones use the
/// language id itself when it is short and alphanumeric.
#[must_use]
pub fn extension_for(artifact_type: &str, language: Option<&str>) -> String {
    if artifact_type == "code" {
        let lang = language.unwrap_or("").trim().to_ascii_lowercase();
        let known = match lang.as_str() {
            "rust" => "rs",
            "python" => "py",
            "typescript" => "ts",
            "javascript" => "js",
            "tsx" | "jsx" | "json" | "toml" | "yaml" | "sql" | "go" | "rb" | "css" | "html"
            | "md" | "sh" | "c" | "h" | "java" | "kt" | "swift" | "cs" | "php" | "lua" | "zig"
            | "ts" | "js" | "rs" | "py" => lang.as_str(),
            "markdown" => "md",
            "shell" | "bash" | "zsh" | "fish" => "sh",
            "csharp" => "cs",
            "kotlin" => "kt",
            "ruby" => "rb",
            "cpp" | "c++" => "cpp",
            "" => "txt",
            other if other.len() <= 5 && other.chars().all(|c| c.is_ascii_alphanumeric()) => other,
            _ => "txt",
        };
        return known.to_owned();
    }
    if artifact_type == "react" && language == Some("jsx") {
        return "jsx".to_owned();
    }
    get(artifact_type)
        .and_then(|t| t.extensions.first().copied())
        .unwrap_or("txt")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_types_and_their_extensions() {
        assert_eq!(
            ids(),
            ["markdown", "code", "svg", "html", "mermaid", "react"]
        );
        assert_eq!(get("react").unwrap().execution, Execution::Sandbox);
        assert_eq!(get("markdown").unwrap().execution, Execution::None);
        assert!(get("table").is_none());
        assert_eq!(extension_for("code", Some("rust")), "rs");
        assert_eq!(extension_for("code", Some("Python")), "py");
        assert_eq!(extension_for("code", None), "txt");
        assert_eq!(extension_for("react", Some("jsx")), "jsx");
        assert_eq!(extension_for("react", None), "tsx");
        assert_eq!(extension_for("mermaid", None), "mmd");
    }
}
