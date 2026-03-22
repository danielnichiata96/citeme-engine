use serde::{Deserialize, Serialize};

/// Detected input format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputFormat {
    /// BibTeX or BibLaTeX (.bib)
    Bibtex,
    /// RIS (.ris)
    Ris,
    /// CSL-JSON (single object or array)
    CslJson,
    /// Unknown format
    Unknown,
}

impl InputFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputFormat::Bibtex => "bibtex",
            InputFormat::Ris => "ris",
            InputFormat::CslJson => "csl-json",
            InputFormat::Unknown => "unknown",
        }
    }
}

/// Auto-detect input format from content.
///
/// Heuristics (applied in order):
/// 1. Starts with `{` or `[` → CSL-JSON (valid JSON object/array)
/// 2. Contains `@article`, `@book`, etc. → BibTeX
/// 3. Contains `TY  -` → RIS
/// 4. Otherwise → Unknown
pub fn detect_format(input: &str) -> InputFormat {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return InputFormat::Unknown;
    }

    // JSON: starts with { or [
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Verify it's actually valid JSON
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return InputFormat::CslJson;
        }
    }

    // BibTeX: look for @type{ pattern
    let lower = trimmed.to_lowercase();
    if lower.contains("@article")
        || lower.contains("@book")
        || lower.contains("@inproceedings")
        || lower.contains("@incollection")
        || lower.contains("@misc")
        || lower.contains("@phdthesis")
        || lower.contains("@mastersthesis")
        || lower.contains("@techreport")
        || lower.contains("@unpublished")
        || lower.contains("@conference")
        || lower.contains("@inbook")
        || lower.contains("@proceedings")
        || lower.contains("@manual")
        || lower.contains("@online")
        || lower.contains("@thesis")
        || lower.contains("@report")
        || lower.contains("@patent")
    {
        return InputFormat::Bibtex;
    }

    // RIS: TY  - at line start
    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with("TY  -") {
            return InputFormat::Ris;
        }
    }

    InputFormat::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_bibtex() {
        assert_eq!(detect_format("@article{key, title={Test}}"), InputFormat::Bibtex);
        assert_eq!(detect_format("@Book{key,\n  author={A}}"), InputFormat::Bibtex);
        assert_eq!(detect_format("  @ARTICLE{key, title={Test}}"), InputFormat::Bibtex);
    }

    #[test]
    fn test_detect_ris() {
        assert_eq!(detect_format("TY  - JOUR\nAU  - Smith\nER  -"), InputFormat::Ris);
        assert_eq!(detect_format("TY  -JOUR\nER  -"), InputFormat::Ris);
    }

    #[test]
    fn test_detect_csl_json_object() {
        assert_eq!(
            detect_format(r#"{"type": "article-journal", "title": "Test"}"#),
            InputFormat::CslJson
        );
    }

    #[test]
    fn test_detect_csl_json_array() {
        assert_eq!(
            detect_format(r#"[{"type": "article-journal"}, {"type": "book"}]"#),
            InputFormat::CslJson
        );
    }

    #[test]
    fn test_detect_empty() {
        assert_eq!(detect_format(""), InputFormat::Unknown);
        assert_eq!(detect_format("   "), InputFormat::Unknown);
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_format("just some random text"), InputFormat::Unknown);
    }

    #[test]
    fn test_detect_invalid_json_not_csl() {
        // Starts with { but isn't valid JSON → not CSL-JSON
        assert_eq!(detect_format("{not valid json"), InputFormat::Unknown);
    }
}
