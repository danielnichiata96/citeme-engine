use std::collections::HashMap;

use hayagriva::citationberg::{IndependentStyle, LocaleFile, Locale, Style};
use hayagriva::{BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest};

use crate::error::EngineError;
use crate::types::{FormatOptions, FormatResult, OutputFormat};

/// Citation formatting engine.
///
/// Holds compiled CSL styles and locale data in memory.
/// Styles are parsed from XML once and reused for all subsequent format calls.
pub struct CitationEngine {
    /// Compiled CSL styles, keyed by style name (e.g., "apa", "abnt")
    styles: HashMap<String, IndependentStyle>,
    /// Loaded locales (used by BibliographyDriver)
    locales: Vec<Locale>,
    /// Track which locale codes are loaded to avoid duplicates
    loaded_locale_codes: Vec<String>,
}

impl CitationEngine {
    /// Create a new, empty engine. Call `load_style` and `load_locale` before formatting.
    pub fn new() -> Self {
        Self {
            styles: HashMap::new(),
            locales: Vec::new(),
            loaded_locale_codes: Vec::new(),
        }
    }

    /// Load and compile a CSL style from XML.
    /// Cached — subsequent calls with the same name are no-ops.
    pub fn load_style(&mut self, name: &str, csl_xml: &str) -> Result<(), EngineError> {
        if self.styles.contains_key(name) {
            return Ok(());
        }

        let style = Style::from_xml(csl_xml)
            .map_err(|e| EngineError::InvalidStyle(format!("{e}")))?;

        match style {
            Style::Independent(ind) => {
                self.styles.insert(name.to_string(), ind);
                Ok(())
            }
            Style::Dependent(_) => {
                Err(EngineError::InvalidStyle(
                    "dependent styles are not supported — load the parent style instead".into(),
                ))
            }
        }
    }

    /// Load a locale from XML.
    pub fn load_locale(&mut self, locale_code: &str, locale_xml: &str) -> Result<(), EngineError> {
        if self.loaded_locale_codes.contains(&locale_code.to_string()) {
            return Ok(());
        }

        let locale_file = LocaleFile::from_xml(locale_xml)
            .map_err(|e| EngineError::InvalidLocale(format!("{e}")))?;

        self.locales.push(locale_file.into());
        self.loaded_locale_codes.push(locale_code.to_string());
        Ok(())
    }

    /// Check if a style is loaded.
    pub fn has_style(&self, name: &str) -> bool {
        self.styles.contains_key(name)
    }

    /// List loaded style names.
    pub fn loaded_styles(&self) -> Vec<String> {
        self.styles.keys().cloned().collect()
    }

    /// Resolve a locale code string to a `LocaleCode` for Hayagriva.
    fn resolve_locale(&self, locale_code: &str) -> Option<hayagriva::citationberg::LocaleCode> {
        if locale_code.is_empty() {
            None
        } else {
            Some(hayagriva::citationberg::LocaleCode(locale_code.to_string()))
        }
    }

    /// Format a single CSL-JSON item.
    ///
    /// The CSL-JSON string is deserialized into a `citationberg::json::Item`
    /// and passed directly to Hayagriva's `BibliographyDriver` — no conversion
    /// to Hayagriva `Entry` needed (enabled by the `csl-json` feature).
    pub fn format_one(
        &self,
        csl_json_str: &str,
        style_name: &str,
        locale_code: &str,
        options: &FormatOptions,
    ) -> Result<FormatResult, EngineError> {
        let style = self.styles.get(style_name)
            .ok_or_else(|| EngineError::StyleNotLoaded(style_name.into()))?;

        // Parse CSL-JSON string into citationberg::json::Item
        // This uses the csl-json feature — Item implements EntryLike
        let item: hayagriva::citationberg::json::Item = serde_json::from_str(csl_json_str)
            .map_err(|e| EngineError::InvalidCslJson(format!("{e}")))?;

        let locale = self.resolve_locale(locale_code);

        // Create driver and format
        let mut driver = BibliographyDriver::new();

        let cite_items = vec![CitationItem::with_entry(&item)];
        driver.citation(CitationRequest::new(
            cite_items,
            style,
            locale.clone(),
            &self.locales,
            None,
        ));

        let rendered = driver.finish(BibliographyRequest {
            style,
            locale,
            locale_files: &self.locales,
        });

        // Extract reference (bibliography entry)
        let buf_format = match options.output_format {
            OutputFormat::Html => BufWriteFormat::Html,
            OutputFormat::Plain => BufWriteFormat::Plain,
        };

        let reference = rendered.bibliography
            .and_then(|bib| bib.items.into_iter().next())
            .map(|item| {
                let mut buf = String::new();
                let _ = item.content.write_buf(&mut buf, buf_format);
                buf.trim().to_string()
            })
            .unwrap_or_default();

        // Extract in-text citation
        let in_text = rendered.citations.into_iter().next()
            .map(|c| {
                let mut buf = String::new();
                let _ = c.citation.write_buf(&mut buf, buf_format);
                buf.trim().to_string()
            })
            .unwrap_or_default();

        // Fallback: if Hayagriva produced empty output, build a degraded citation
        if reference.is_empty() && in_text.is_empty() {
            return Ok(Self::build_fallback(csl_json_str));
        }

        Ok(FormatResult { reference, in_text })
    }

    /// Build a minimal citation when the CSL engine produces empty output.
    fn build_fallback(csl_json_str: &str) -> FormatResult {
        let v: serde_json::Value = serde_json::from_str(csl_json_str)
            .unwrap_or_default();

        let author = v["author"].as_array()
            .and_then(|a| a.first())
            .and_then(|a| {
                a["literal"].as_str()
                    .or(a["family"].as_str())
            })
            .or(v["container-title"].as_str())
            .unwrap_or("Unknown");

        let year = v["issued"]["date-parts"].as_array()
            .and_then(|dp| dp.first())
            .and_then(|parts| parts.as_array())
            .and_then(|parts| parts.first())
            .and_then(|y| y.as_i64())
            .map(|y| y.to_string())
            .unwrap_or_else(|| "n.d.".into());

        let title = v["title"].as_str().unwrap_or("Untitled");

        let url = v["URL"].as_str()
            .or_else(|| v["DOI"].as_str().map(|_| ""))
            .unwrap_or("");

        let url_part = if !url.is_empty() {
            format!(" {url}")
        } else if let Some(doi) = v["DOI"].as_str() {
            format!(" https://doi.org/{doi}")
        } else {
            String::new()
        };

        FormatResult {
            reference: format!("{author} ({year}). {title}.{url_part}"),
            in_text: format!("({author}, {year})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CSL: &str = include_str!("../../../tests/fixtures/styles/apa.csl");
    const SAMPLE_LOCALE: &str = include_str!("../../../tests/fixtures/locales/locales-en-US.xml");

    #[test]
    fn test_load_style_and_locale() {
        let mut engine = CitationEngine::new();

        assert!(!engine.has_style("apa"));

        engine.load_style("apa", SAMPLE_CSL).unwrap();
        assert!(engine.has_style("apa"));

        // Loading same style again is a no-op
        engine.load_style("apa", SAMPLE_CSL).unwrap();

        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();
    }

    #[test]
    fn test_load_invalid_style() {
        let mut engine = CitationEngine::new();
        let result = engine.load_style("bad", "<not-valid-csl/>");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_one_apa() {
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json = r#"{
            "type": "article-journal",
            "id": "smith2024",
            "title": "A Study of Something",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2024]]},
            "container-title": "Journal of Testing",
            "volume": "42",
            "issue": "3",
            "page": "100-115",
            "DOI": "10.1234/test.2024"
        }"#;

        let opts = FormatOptions::default();
        let result = engine.format_one(csl_json, "apa", "en-US", &opts).unwrap();

        assert!(result.reference.contains("Smith"), "reference should contain author: {}", result.reference);
        assert!(result.reference.contains("2024"), "reference should contain year: {}", result.reference);
        assert!(result.reference.contains("A Study of Something"), "reference should contain title: {}", result.reference);
        // Hayagriva uses <span style="font-style: italic;"> rather than <i>
        assert!(result.reference.contains("font-style: italic"), "APA reference should contain italics (HTML mode): {}", result.reference);
        assert!(!result.reference.is_empty(), "reference should not be empty");

        assert!(result.in_text.contains("Smith"), "in_text should contain author: {}", result.in_text);
        assert!(result.in_text.contains("2024"), "in_text should contain year: {}", result.in_text);
        assert!(result.in_text.contains('(') && result.in_text.contains(')'),
            "APA in_text should have parentheses: {}", result.in_text);
    }

    #[test]
    fn test_format_one_style_not_loaded() {
        let engine = CitationEngine::new();
        let result = engine.format_one("{}", "nonexistent", "en-US", &FormatOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_format_one_fallback_on_empty_output() {
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json = r#"{
            "type": "article-journal",
            "title": "Some Title",
            "author": [{"family": "Author"}],
            "issued": {"date-parts": [[2024]]}
        }"#;

        let result = engine.format_one(csl_json, "apa", "en-US", &FormatOptions::default()).unwrap();
        assert!(!result.reference.is_empty(), "reference must never be empty");
        assert!(!result.in_text.is_empty(), "in_text must never be empty");
    }
}
