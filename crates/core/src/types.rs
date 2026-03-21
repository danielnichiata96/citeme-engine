use serde::{Deserialize, Serialize};

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
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Html,
            abnt_post_process: false,
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
