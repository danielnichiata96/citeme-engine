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
    /// MEDLINE/NBIB (PubMed export)
    Medline,
    /// Unknown format
    Unknown,
}

impl InputFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputFormat::Bibtex => "bibtex",
            InputFormat::Ris => "ris",
            InputFormat::CslJson => "csl-json",
            InputFormat::Medline => "medline",
            InputFormat::Unknown => "unknown",
        }
    }
}

/// `@` + ASCII-alpha entry type + optional whitespace + `{` or `(`.
/// Bare `@` in prose ("me @ home", "@user mentions") never matches: the
/// type word must be non-empty and immediately followed by the opener.
fn looks_like_bibtex_entry(s: &str) -> bool {
    for (i, c) in s.char_indices() {
        if c != '@' {
            continue;
        }
        let rest = &s[i + 1..];
        // ASCII-alpha chars are 1 byte each, so count == byte offset.
        let word_len = rest.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        if word_len == 0 {
            continue;
        }
        let after = rest[word_len..].trim_start();
        if after.starts_with('{') || after.starts_with('(') {
            return true;
        }
    }
    false
}

/// Auto-detect input format from content.
///
/// Heuristics (applied in order):
/// 1. Starts with `{` or `[` → CSL-JSON (valid JSON object/array)
/// 2. Any `@word{` / `@word(` entry opener → BibTeX
/// 3. Contains `TY  -` at line start → RIS
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

    // BibTeX: any `@word{` / `@word(` entry opener. A closed list of entry
    // types silently dropped valid BibTeX (`@dataset`, `@software`, custom
    // types biblatex accepts) into Unknown.
    if looks_like_bibtex_entry(trimmed) {
        return InputFormat::Bibtex;
    }

    // RIS: TY  - at line start
    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with("TY  -") {
            return InputFormat::Ris;
        }
    }

    // MEDLINE/NBIB: PMID- at line start, or FAU - / TI  - pattern
    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with("PMID-") || line.starts_with("FAU -") || line.starts_with("AU  -") {
            // Distinguish from RIS: MEDLINE uses "PMID-" and "FAU -", RIS uses "TY  -"
            // If we got here, TY was not found, so it's MEDLINE not RIS
            return InputFormat::Medline;
        }
    }

    InputFormat::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_bibtex() {
        assert_eq!(
            detect_format("@article{key, title={Test}}"),
            InputFormat::Bibtex
        );
        assert_eq!(
            detect_format("@Book{key,\n  author={A}}"),
            InputFormat::Bibtex
        );
        assert_eq!(
            detect_format("  @ARTICLE{key, title={Test}}"),
            InputFormat::Bibtex
        );
    }

    #[test]
    fn test_detect_ris() {
        assert_eq!(
            detect_format("TY  - JOUR\nAU  - Smith\nER  -"),
            InputFormat::Ris
        );
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
    fn test_detect_medline() {
        assert_eq!(
            detect_format("PMID- 12345678\nTI  - A Study\nFAU - Smith, John\n"),
            InputFormat::Medline
        );
        assert_eq!(
            detect_format("FAU - Smith, John\nTI  - Test\n"),
            InputFormat::Medline
        );
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
