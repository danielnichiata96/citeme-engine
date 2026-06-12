use serde::{Deserialize, Serialize};

/// Version of every JSON shape returned across the public API boundary.
///
/// Consumers deserialize the engine's JSON output against their own schemas
/// (the CiteMe app validates with Zod per call). This constant lets them
/// assert compatibility ONCE at engine init — drift becomes a loud boot
/// failure instead of per-call validation noise.
///
/// Covered shapes (bump this when any of them changes incompatibly —
/// renamed/removed/retyped fields; adding a NEW OPTIONAL field is not a bump
/// for tolerant readers, but strict-schema consumers should be notified in
/// the changelog):
/// - `FormatResult` → `{ "reference": string, "inText": string }`
///   (and arrays of it from the batch methods)
/// - `ParseResult` → `{ "entries": object[], "errors": {"preview","error"}[],
///   "format": string, "truncated": bool, "scannedEntries": number }`
///
/// The Wasm boundary exposes this as `resultShapeVersion()`.
pub const RESULT_SHAPE_VERSION: u32 = 1;

/// Output of a citation formatting operation.
/// Matches CiteMe's `SimpleCitation` type in TypeScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatResult {
    /// Full bibliography entry (e.g., "Smith, J. (2023). Title...")
    pub reference: String,
    /// In-text citation (e.g., "(Smith, 2023)" or "[1]")
    pub in_text: String,
}

/// Options for formatting operations.
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// Output format for the formatted strings
    pub output_format: OutputFormat,
    /// Apply ABNT 2023 post-processing
    pub abnt_post_process: bool,
    /// Use prose/narrative citation form ("Smith (2024)" instead of "(Smith, 2024)")
    pub prose: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Html,
            abnt_post_process: false,
            prose: false,
        }
    }
}

/// Output format for rendered citations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// HTML with `<i>`, `<b>`, `<span>` tags (matches citation-js default)
    Html,
    /// Plain text (no markup)
    Plain,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Pins the exact serialized shape covered by RESULT_SHAPE_VERSION.
    /// If this test needs editing, RESULT_SHAPE_VERSION must be bumped and
    /// the change called out in the CHANGELOG.
    #[test]
    fn format_result_shape_is_pinned_to_shape_version() {
        let result = FormatResult {
            reference: "ref".into(),
            in_text: "txt".into(),
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({ "reference": "ref", "inText": "txt" }),
            "FormatResult JSON shape changed — bump RESULT_SHAPE_VERSION"
        );
        assert_eq!(RESULT_SHAPE_VERSION, 1);
    }

    /// Same pin for ParseResult (returned by all parse* boundary methods).
    #[test]
    fn parse_result_shape_is_pinned_to_shape_version() {
        let result = crate::parsers::ParseResult {
            entries: vec![json!({"type": "book"})],
            errors: vec![crate::parsers::ParseErrorInfo {
                preview: "p".into(),
                error: "e".into(),
            }],
            format: "bibtex".into(),
            truncated: false,
            scanned_entries: 1,
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({
                "entries": [{"type": "book"}],
                "errors": [{"preview": "p", "error": "e"}],
                "format": "bibtex",
                "truncated": false,
                "scannedEntries": 1
            }),
            "ParseResult JSON shape changed — bump RESULT_SHAPE_VERSION"
        );
    }
}
