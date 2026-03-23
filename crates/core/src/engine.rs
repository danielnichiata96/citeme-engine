use std::collections::HashMap;

use hayagriva::citationberg::{IndependentStyle, LocaleFile, Locale, Style};
use hayagriva::{BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, CitePurpose};

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

        let mut cite_item = CitationItem::with_entry(&item);
        if options.prose {
            cite_item = cite_item.kind(CitePurpose::Prose);
        }
        let cite_items = vec![cite_item];
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
            .map(|item| Self::render_bib_item(&item, buf_format))
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
            let fallback = Self::build_fallback(csl_json_str);
            return Ok(Self::apply_abnt_if_needed(fallback, options));
        }

        Ok(Self::apply_abnt_if_needed(FormatResult { reference, in_text }, options))
    }

    /// Format a batch of CSL-JSON items in one call.
    ///
    /// Input: JSON string containing an array of CSL-JSON items.
    /// Uses ONE shared BibliographyDriver for the entire batch so that:
    /// - Numeric styles (IEEE) get correct sequential numbering
    /// - Author-year styles with disambiguation (APA) resolve correctly
    /// - Citation grouping works (e.g., "(Smith, 2024a; Smith, 2024b)")
    pub fn format_batch(
        &self,
        csl_json_array_str: &str,
        style_name: &str,
        locale_code: &str,
        options: &FormatOptions,
    ) -> Result<Vec<FormatResult>, EngineError> {
        let style = self.styles.get(style_name)
            .ok_or_else(|| EngineError::StyleNotLoaded(style_name.into()))?;

        let mut items: Vec<serde_json::Value> = serde_json::from_str(csl_json_array_str)
            .map_err(|e| EngineError::InvalidCslJson(format!("{e}")))?;

        if items.is_empty() {
            return Ok(vec![]);
        }

        // Inject synthetic ids for items that lack them, so the bibliography
        // key-based lookup works correctly for all items.
        for (i, item) in items.iter_mut().enumerate() {
            if let Some(obj) = item.as_object_mut() {
                if !obj.contains_key("id") || obj["id"].as_str().unwrap_or("").is_empty() {
                    obj.insert("id".to_string(), serde_json::Value::String(format!("_citeme_batch_{i}")));
                }
            }
        }

        let locale = self.resolve_locale(locale_code);
        let buf_format = match options.output_format {
            OutputFormat::Html => BufWriteFormat::Html,
            OutputFormat::Plain => BufWriteFormat::Plain,
        };

        // Parse all items
        let parsed_items: Vec<hayagriva::citationberg::json::Item> = items.iter()
            .map(|v| serde_json::from_value(v.clone())
                .map_err(|e| EngineError::InvalidCslJson(format!("{e}"))))
            .collect::<Result<Vec<_>, _>>()?;

        // Use ONE shared driver for the entire batch
        let mut driver = BibliographyDriver::new();

        for item in &parsed_items {
            let mut cite_item = CitationItem::with_entry(item);
            if options.prose {
                cite_item = cite_item.kind(CitePurpose::Prose);
            }
            let cite_items = vec![cite_item];
            driver.citation(CitationRequest::new(
                cite_items,
                style,
                locale.clone(),
                &self.locales,
                None,
            ));
        }

        let rendered = driver.finish(BibliographyRequest {
            style,
            locale,
            locale_files: &self.locales,
        });

        // Build bibliography key-based lookup.
        // All items now have ids (synthetic ones injected above for items without).
        let bib_map: HashMap<String, String> = rendered.bibliography
            .map(|bib| bib.items.iter().map(|bib_item| {
                (bib_item.key.clone(), Self::render_bib_item(bib_item, buf_format))
            }).collect())
            .unwrap_or_default();

        // Match each citation to its bibliography entry by key
        let mut results = Vec::with_capacity(parsed_items.len());

        for (i, cite) in rendered.citations.iter().enumerate() {
            let item_key = parsed_items[i].id()
                .map(|cow| cow.into_owned())
                .unwrap_or_else(|| format!("_citeme_batch_{i}"));

            let reference = bib_map.get(&item_key).cloned().unwrap_or_default();

            let in_text = {
                let mut buf = String::new();
                let _ = cite.citation.write_buf(&mut buf, buf_format);
                buf.trim().to_string()
            };

            let result = if reference.is_empty() && in_text.is_empty() {
                let item_json = serde_json::to_string(&items[i]).unwrap_or_default();
                Self::build_fallback(&item_json)
            } else {
                FormatResult { reference, in_text }
            };
            results.push(Self::apply_abnt_if_needed(result, options));
        }

        Ok(results)
    }

    /// Render a BibliographyItem to string, including first_field prefix
    /// (numbering like "[1]" or "1." for numeric styles like IEEE/Vancouver).
    fn render_bib_item(item: &hayagriva::BibliographyItem, buf_format: BufWriteFormat) -> String {
        let mut buf = String::new();
        // first_field contains the numbering prefix for numeric styles
        if let Some(ref prefix) = item.first_field {
            let _ = prefix.write_buf(&mut buf, buf_format);
            // Add space between prefix and content if prefix doesn't end with space
            if !buf.is_empty() && !buf.ends_with(' ') {
                buf.push(' ');
            }
        }
        let _ = item.content.write_buf(&mut buf, buf_format);
        buf.trim().to_string()
    }

    /// Apply ABNT post-processing if the options flag is set.
    fn apply_abnt_if_needed(result: FormatResult, options: &FormatOptions) -> FormatResult {
        if options.abnt_post_process {
            FormatResult {
                reference: crate::abnt::post_process_abnt(&result.reference, false),
                in_text: crate::abnt::post_process_abnt(&result.in_text, true),
            }
        } else {
            result
        }
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

        let url_part = if let Some(url) = v["URL"].as_str() {
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

    #[test]
    fn test_format_batch() {
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json_array = r#"[
            {
                "type": "article-journal",
                "id": "smith2024",
                "title": "First Paper",
                "author": [{"family": "Smith", "given": "John"}],
                "issued": {"date-parts": [[2024]]}
            },
            {
                "type": "book",
                "id": "doe2023",
                "title": "A Great Book",
                "author": [{"family": "Doe", "given": "Jane"}],
                "issued": {"date-parts": [[2023]]},
                "publisher": "Academic Press"
            }
        ]"#;

        let opts = FormatOptions::default();
        let results = engine.format_batch(csl_json_array, "apa", "en-US", &opts).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].reference.contains("Smith"));
        assert!(results[1].reference.contains("Doe"));
    }

    #[test]
    fn test_format_batch_empty() {
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let results = engine.format_batch("[]", "apa", "en-US", &FormatOptions::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_format_one_abnt_uppercase() {
        let abnt_csl = include_str!("../../../tests/fixtures/styles/abnt.csl");
        let pt_br_locale = include_str!("../../../tests/fixtures/locales/locales-pt-BR.xml");

        let mut engine = CitationEngine::new();
        engine.load_style("abnt", abnt_csl).unwrap();
        engine.load_locale("pt-BR", pt_br_locale).unwrap();

        let csl_json = r#"{
            "type": "article-journal",
            "title": "Um estudo importante",
            "author": [{"family": "Silva", "given": "João"}],
            "issued": {"date-parts": [[2024]]},
            "container-title": "Revista Brasileira de Testes"
        }"#;

        let opts = FormatOptions {
            abnt_post_process: true,
            ..Default::default()
        };

        let result = engine.format_one(csl_json, "abnt", "pt-BR", &opts).unwrap();
        assert!(result.reference.contains("SILVA"), "ABNT reference should have uppercased family name: {}", result.reference);
    }

    #[test]
    fn test_format_batch_items_without_id() {
        // Regression: items without "id" field should still get bibliography output
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json_array = r#"[
            {
                "type": "article-journal",
                "title": "First Paper No Id",
                "author": [{"family": "Smith", "given": "John"}],
                "issued": {"date-parts": [[2024]]}
            },
            {
                "type": "book",
                "title": "Second Book No Id",
                "author": [{"family": "Doe", "given": "Jane"}],
                "issued": {"date-parts": [[2023]]}
            }
        ]"#;

        let opts = FormatOptions::default();
        let results = engine.format_batch(csl_json_array, "apa", "en-US", &opts).unwrap();

        assert_eq!(results.len(), 2);
        assert!(!results[0].reference.is_empty(), "first item reference should not be empty: {:?}", results[0]);
        assert!(!results[1].reference.is_empty(), "second item reference should not be empty: {:?}", results[1]);
        assert!(results[0].reference.contains("Smith"), "first reference should contain Smith: {}", results[0].reference);
        assert!(results[1].reference.contains("Doe"), "second reference should contain Doe: {}", results[1].reference);
    }

    #[test]
    fn test_format_one_prose_citation() {
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
            "volume": "42"
        }"#;

        let prose_opts = FormatOptions {
            prose: true,
            ..Default::default()
        };
        let result = engine.format_one(csl_json, "apa", "en-US", &prose_opts).unwrap();

        // Prose citation: "Smith (2024)" — author outside parens, year inside
        assert!(!result.in_text.is_empty(), "prose in_text must not be empty");
        assert!(result.in_text.contains("Smith"), "prose in_text should contain author: {}", result.in_text);
        assert!(result.in_text.contains("2024"), "prose in_text should contain year: {}", result.in_text);
        // Positive: prose form starts with bare author name
        let stripped = result.in_text.replace("<span", "").replace("</span>", "");
        assert!(!stripped.trim_start().starts_with('('),
            "prose in_text should start with bare author, not '(' — got: {}", result.in_text);
    }

    #[test]
    fn test_format_batch_prose_citation() {
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json_array = r#"[
            {
                "type": "article-journal",
                "id": "smith2024",
                "title": "First Paper",
                "author": [{"family": "Smith", "given": "John"}],
                "issued": {"date-parts": [[2024]]}
            },
            {
                "type": "book",
                "id": "doe2023",
                "title": "A Great Book",
                "author": [{"family": "Doe", "given": "Jane"}],
                "issued": {"date-parts": [[2023]]}
            }
        ]"#;

        let prose_opts = FormatOptions {
            prose: true,
            ..Default::default()
        };
        let results = engine.format_batch(csl_json_array, "apa", "en-US", &prose_opts).unwrap();

        assert_eq!(results.len(), 2);
        // Both must be non-empty
        assert!(!results[0].in_text.is_empty(), "first prose citation must not be empty");
        assert!(!results[1].in_text.is_empty(), "second prose citation must not be empty");
        // Positive: contains author names
        assert!(results[0].in_text.contains("Smith"),
            "first prose citation should contain Smith: {}", results[0].in_text);
        assert!(results[1].in_text.contains("Doe"),
            "second prose citation should contain Doe: {}", results[1].in_text);
        // Negative: should not be parenthetical form
        let s0 = results[0].in_text.replace("<span", "").replace("</span>", "");
        let s1 = results[1].in_text.replace("<span", "").replace("</span>", "");
        assert!(!s0.trim_start().starts_with('('),
            "first prose citation should not be parenthetical: {}", results[0].in_text);
        assert!(!s1.trim_start().starts_with('('),
            "second prose citation should not be parenthetical: {}", results[1].in_text);
    }
}
