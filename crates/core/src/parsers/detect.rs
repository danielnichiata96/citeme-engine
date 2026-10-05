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

/// A byte-order mark: not content, but Windows exporters prefix one.
const BOM: char = '\u{FEFF}';

/// `s` opens with `@` + ASCII-alpha entry type + optional whitespace + `{`
/// or `(`. Bare `@` in prose ("me @ home", "@user mentions") never matches:
/// the type word must be non-empty and immediately followed by the opener.
fn starts_bibtex_entry(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('@') else {
        return false;
    };
    // ASCII-alpha chars are 1 byte each, so count == byte offset.
    let word_len = rest.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    if word_len == 0 {
        return false;
    }
    let after = rest[word_len..].trim_start();
    after.starts_with('{') || after.starts_with('(')
}

/// An entry opener anywhere in `s`.
fn looks_like_bibtex_entry(s: &str) -> bool {
    s.char_indices()
        .any(|(i, c)| c == '@' && starts_bibtex_entry(&s[i..]))
}

/// A RIS/MEDLINE tag line: an uppercase tag ("TY", "PMID", "A2"), optional
/// spaces, then a dash ("TY  - JOUR", "PMID- 1", "TI - x").
fn is_tag_line(line: &str) -> bool {
    let tag_len = line
        .bytes()
        .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        .count();
    line.starts_with(|c: char| c.is_ascii_uppercase())
        && (2..=4).contains(&tag_len)
        && line[tag_len..].trim_start_matches(' ').starts_with('-')
}

/// Auto-detect input format from content.
///
/// Heuristics (applied in order):
/// 1. Starts with `{` or `[` → CSL-JSON (valid JSON object/array)
/// 2. The first line that opens a record decides: an `@word{` / `@word(`
///    entry opener → BibTeX; a tag line → RIS when any line starts with
///    `TY  -`, MEDLINE when one starts with `PMID-`, `FAU -` or `AU  -`
/// 3. An `@word{` / `@word(` entry opener anywhere → BibTeX
/// 4. Otherwise → Unknown
pub fn detect_format(input: &str) -> InputFormat {
    let trimmed = input.trim_start_matches(BOM).trim();
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

    // Each format opens its records at the start of a line, so the first
    // line that does is the file's format. An opener inside a field comes
    // later: "@WHO (" in a RIS abstract matched the BibTeX opener and the
    // whole file was parsed as BibTeX, importing nothing.
    let lines = || {
        trimmed
            .lines()
            .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == BOM))
    };
    for line in lines() {
        if starts_bibtex_entry(line) {
            return InputFormat::Bibtex;
        }
        if is_tag_line(line) {
            if lines().any(|line| line.starts_with("TY  -")) {
                return InputFormat::Ris;
            }
            // MEDLINE/NBIB: "PMID-" and "FAU -" are MEDLINE's own; "AU  -"
            // is shared with RIS, but without a TY line it isn't RIS.
            if lines().any(|line| {
                line.starts_with("PMID-") || line.starts_with("FAU -") || line.starts_with("AU  -")
            }) {
                return InputFormat::Medline;
            }
            break;
        }
    }

    // BibTeX: any `@word{` / `@word(` entry opener, even after some prose
    // ("Here it is: @article{…}"). A closed list of entry types silently
    // dropped valid BibTeX (`@dataset`, `@software`, custom types biblatex
    // accepts) into Unknown.
    if looks_like_bibtex_entry(trimmed) {
        return InputFormat::Bibtex;
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
    fn test_detect_at_mention_in_ris_or_medline_text_is_not_bibtex() {
        // "@WHO (" inside an abstract matched the `@word(` entry opener, so
        // the whole file was parsed as BibTeX and imported nothing.
        assert_eq!(
            detect_format(
                "TY  - JOUR\nTI  - T\nAB  - Guidance from @WHO (World Health Organization) was followed.\nER  - \n"
            ),
            InputFormat::Ris
        );
        assert_eq!(
            detect_format(
                "PMID- 1\nTI  - T.\nAB  - Data shared by @CDCgov (Centers for Disease Control).\nFAU - Smith, John\n"
            ),
            InputFormat::Medline
        );
        // PubMed wraps at ~80 columns: the mention can open a wrapped line.
        assert_eq!(
            detect_format(
                "PMID- 1\nAB  - Data shared by\n      @CDCgov (Centers for Disease Control).\n"
            ),
            InputFormat::Medline
        );
    }

    #[test]
    fn test_detect_bibtex_entry_opener_anywhere() {
        // A pasted entry after some prose is still BibTeX, and a BibTeX file
        // whose fields mention RIS/MEDLINE tags stays BibTeX.
        assert_eq!(
            detect_format("Here it is: @article{k, title={T}}"),
            InputFormat::Bibtex
        );
        assert_eq!(
            detect_format("@article{k,\n  note = {exported as\nPMID- 123},\n  title = {T}\n}"),
            InputFormat::Bibtex
        );
    }

    #[test]
    fn test_detect_sees_past_a_utf8_bom() {
        assert_eq!(
            detect_format("\u{FEFF}TY  - JOUR\nTI  - First\nER  - \n"),
            InputFormat::Ris
        );
        assert_eq!(
            detect_format("\u{FEFF}PMID- 1\nTI  - T.\n"),
            InputFormat::Medline
        );
        assert_eq!(
            detect_format("\u{FEFF}@article{k, title={T}}"),
            InputFormat::Bibtex
        );
        assert_eq!(
            detect_format("\u{FEFF}[{\"type\": \"book\"}]"),
            InputFormat::CslJson
        );
    }

    #[test]
    fn test_detect_invalid_json_not_csl() {
        // Starts with { but isn't valid JSON → not CSL-JSON
        assert_eq!(detect_format("{not valid json"), InputFormat::Unknown);
    }
}
