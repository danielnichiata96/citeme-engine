pub mod bibtex;
pub mod csl_json;
pub mod detect;
pub mod medline;
pub mod ris;

use serde::{Deserialize, Serialize};

/// Hard cap on collected parse errors. `max_entries` bounds successful
/// output but not the error array — hostile input full of broken entries
/// would otherwise balloon the result (each error carries an 80-char
/// preview). Consumers only surface the first few errors anyway.
pub const MAX_PARSE_ERRORS: usize = 100;

/// Options for import parsers (includes DoS guards per spec §5.1).
#[derive(Debug, Clone)]
pub struct ParseOptions {
    /// Maximum entries to parse. None = unlimited.
    pub max_entries: Option<usize>,
    /// Maximum input size in bytes. Default: 10 MB.
    pub max_input_bytes: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_entries: None,
            max_input_bytes: 10 * 1024 * 1024, // 10 MB
        }
    }
}

/// Result from an import parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    /// Successfully parsed entries as CSL-JSON (serde_json::Value)
    pub entries: Vec<serde_json::Value>,
    /// Entries that failed to parse
    pub errors: Vec<ParseErrorInfo>,
    /// Detected format
    pub format: String,
    /// True if parsing stopped early due to limits
    pub truncated: bool,
    /// Number of entry blocks scanned
    pub scanned_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseErrorInfo {
    pub preview: String,
    pub error: String,
}

impl ParseResult {
    /// A refusal: no entries, one explanatory error, marked truncated.
    fn refused(format: &str, input: &str, error: &str) -> Self {
        Self {
            entries: vec![],
            errors: vec![ParseErrorInfo {
                preview: input.chars().take(80).collect(),
                error: error.to_string(),
            }],
            format: format.to_string(),
            truncated: true,
            scanned_entries: 0,
        }
    }
}

/// Detect the input format and parse it.
///
/// The size guard runs **before** detection on purpose: proving that input is
/// JSON means deserializing all of it into a `Value`, so detecting first let a
/// document far over `max_input_bytes` be fully allocated and parsed before
/// any per-format parser got the chance to refuse it.
pub fn parse_auto(input: &str, options: &ParseOptions) -> ParseResult {
    if input.len() > options.max_input_bytes {
        return ParseResult::refused(
            "unknown",
            input,
            &format!(
                "input too large: {} bytes (max {})",
                input.len(),
                options.max_input_bytes
            ),
        );
    }

    let format = detect::detect_format(input);
    let result = match format {
        detect::InputFormat::Bibtex => bibtex::parse_bibtex(input, options),
        detect::InputFormat::Ris => ris::parse_ris(input, options),
        detect::InputFormat::CslJson => csl_json::parse_csl_json(input, options),
        detect::InputFormat::Medline => medline::parse_medline(input, options),
        detect::InputFormat::Unknown => {
            return ParseResult {
                truncated: false,
                ..ParseResult::refused("unknown", input, "could not detect format")
            }
        }
    };

    // A detected format that yields nothing and says nothing is the worst
    // outcome for a consumer: indistinguishable from "the file was empty".
    if result.entries.is_empty() && result.errors.is_empty() && !input.trim().is_empty() {
        return ParseResult {
            errors: vec![ParseErrorInfo {
                preview: input.chars().take(80).collect(),
                error: format!(
                    "detected {} but no entries could be read from it",
                    result.format
                ),
            }],
            ..result
        };
    }

    result
}
