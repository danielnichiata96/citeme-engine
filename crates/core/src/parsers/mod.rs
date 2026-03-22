pub mod bibtex;
pub mod csl_json;
pub mod ris;

use serde::{Deserialize, Serialize};

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
