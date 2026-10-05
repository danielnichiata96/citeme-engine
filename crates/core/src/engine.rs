use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use hayagriva::citationberg::{IndependentStyle, Locale, LocaleFile, Style};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest,
    CitePurpose,
};

use crate::csl_item::{self, ItemProblem};
use crate::error::EngineError;
use crate::normalize::normalize_csl_xml;
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

impl Default for CitationEngine {
    fn default() -> Self {
        Self::new()
    }
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

        let normalized = normalize_csl_xml(csl_xml);
        let style =
            Style::from_xml(&normalized).map_err(|e| EngineError::InvalidStyle(format!("{e}")))?;

        match style {
            Style::Independent(ind) => {
                self.styles.insert(name.to_string(), ind);
                Ok(())
            }
            Style::Dependent(_) => Err(EngineError::InvalidStyle(
                "dependent styles are not supported — load the parent style instead".into(),
            )),
        }
    }

    /// Load a locale from XML.
    pub fn load_locale(&mut self, locale_code: &str, locale_xml: &str) -> Result<(), EngineError> {
        if self.has_locale(locale_code) {
            return Ok(());
        }

        let normalized = normalize_csl_xml(locale_xml);
        let locale_file = LocaleFile::from_xml(&normalized)
            .map_err(|e| EngineError::InvalidLocale(format!("{e}")))?;

        self.locales.push(locale_file.into());
        self.loaded_locale_codes.push(locale_code.to_string());
        Ok(())
    }

    /// Check if a style is loaded.
    pub fn has_style(&self, name: &str) -> bool {
        self.styles.contains_key(name)
    }

    /// Check if a locale is loaded.
    pub fn has_locale(&self, locale_code: &str) -> bool {
        self.loaded_locale_codes
            .iter()
            .any(|code| code == locale_code)
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

    /// Reject format calls that name a locale we never loaded.
    ///
    /// Hayagriva renders happily with an unresolvable locale, silently
    /// dropping every locale term (long month names, "and", "n.d.", …) —
    /// e.g. "Brevet déposé le 3 2 2021publié…" instead of
    /// "…le 3 février 2021 et publié…". That degraded output looks like a
    /// rendering bug but is a wiring bug; fail loudly instead. The empty
    /// string stays permitted as the explicit "use the style's
    /// default-locale against whatever is loaded" escape hatch.
    fn ensure_locale_available(&self, locale_code: &str) -> Result<(), EngineError> {
        if locale_code.is_empty() || self.has_locale(locale_code) {
            Ok(())
        } else {
            Err(EngineError::LocaleNotLoaded(locale_code.into()))
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
        let style = self
            .styles
            .get(style_name)
            .ok_or_else(|| EngineError::StyleNotLoaded(style_name.into()))?;
        self.ensure_locale_available(locale_code)?;

        // Parse, prepare (see `csl_item`) and read into citationberg's
        // `json::Item`, which the csl-json feature makes an `EntryLike`.
        let value: serde_json::Value = serde_json::from_str(csl_json_str)
            .map_err(|e| EngineError::InvalidCslJson(format!("{e}")))?;
        let value = csl_item::prepare(&value).map_err(|p| item_error(p, None))?;
        let item = hayagriva::citationberg::json::Item::deserialize(&value)
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

        let reference = rendered
            .bibliography
            .and_then(|bib| bib.items.into_iter().next())
            .map(|item| Self::render_bib_item(&item, buf_format))
            .unwrap_or_default();

        // Extract in-text citation
        let in_text = rendered
            .citations
            .into_iter()
            .next()
            .map(|c| {
                let mut buf = String::new();
                let _ = c.citation.write_buf(&mut buf, buf_format);
                buf.trim().to_string()
            })
            .unwrap_or_default();

        // Fallback: if Hayagriva produced empty output, build a degraded citation
        if reference.is_empty() && in_text.is_empty() {
            let fallback = Self::build_fallback(csl_json_str);
            return Ok(Self::apply_abnt_if_needed(fallback, options, &value));
        }

        Ok(Self::apply_abnt_if_needed(
            FormatResult { reference, in_text },
            options,
            &value,
        ))
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
        let style = self
            .styles
            .get(style_name)
            .ok_or_else(|| EngineError::StyleNotLoaded(style_name.into()))?;
        self.ensure_locale_available(locale_code)?;

        let items: Vec<serde_json::Value> = serde_json::from_str(csl_json_array_str)
            .map_err(|e| EngineError::InvalidCslJson(format!("{e}")))?;
        let items = items
            .iter()
            .enumerate()
            .map(|(i, item)| csl_item::prepare(item).map_err(|p| item_error(p, Some(i))))
            .collect::<Result<Vec<_>, _>>()?;

        if items.is_empty() {
            return Ok(vec![]);
        }

        let (entries, cited) = batch_entries(items);

        let locale = self.resolve_locale(locale_code);
        let buf_format = match options.output_format {
            OutputFormat::Html => BufWriteFormat::Html,
            OutputFormat::Plain => BufWriteFormat::Plain,
        };

        // Deserialize each entry once, from &Value (`entries` stays owned
        // for the rare empty-output fallback below).
        let parsed_entries: Vec<hayagriva::citationberg::json::Item> = entries
            .iter()
            .map(|v| {
                hayagriva::citationberg::json::Item::deserialize(v)
                    .map_err(|e| EngineError::InvalidCslJson(format!("{e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Use ONE shared driver for the entire batch
        let mut driver = BibliographyDriver::new();

        for &entry in &cited {
            let mut cite_item = CitationItem::with_entry(&parsed_entries[entry]);
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

        // Build bibliography key-based lookup; every entry has a unique id.
        let bib_map: HashMap<String, String> = rendered
            .bibliography
            .map(|bib| {
                bib.items
                    .iter()
                    .map(|bib_item| {
                        (
                            bib_item.key.clone(),
                            Self::render_bib_item(bib_item, buf_format),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Match each citation to its bibliography entry by key
        let mut results = Vec::with_capacity(cited.len());

        for (cite, &entry) in rendered.citations.iter().zip(&cited) {
            let item = &entries[entry];
            let reference = item["id"]
                .as_str()
                .and_then(|id| bib_map.get(id))
                .cloned()
                .unwrap_or_default();

            let in_text = {
                let mut buf = String::new();
                let _ = cite.citation.write_buf(&mut buf, buf_format);
                buf.trim().to_string()
            };

            let result = if reference.is_empty() && in_text.is_empty() {
                Self::build_fallback(&item.to_string())
            } else {
                FormatResult { reference, in_text }
            };
            results.push(Self::apply_abnt_if_needed(result, options, item));
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

    /// Apply ABNT post-processing if the options flag is set and the entry
    /// is led by a name.
    ///
    /// With no author, editor, translator or collection editor, the ABNT
    /// styles lead with the title instead — and uppercasing it as a family
    /// name printed "PESQUISA NACIONAL POR AMOSTRA DE DOMICÍLIOS, síntese de
    /// indicadores" and "(MANUAL DE REDAÇÃO, 2010)".
    fn apply_abnt_if_needed(
        result: FormatResult,
        options: &FormatOptions,
        item: &serde_json::Value,
    ) -> FormatResult {
        let led_by_name = ["author", "editor", "translator", "collection-editor"]
            .iter()
            .any(|variable| item[variable].as_array().is_some_and(|n| !n.is_empty()));
        if options.abnt_post_process && led_by_name {
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
        let v: serde_json::Value = serde_json::from_str(csl_json_str).unwrap_or_default();

        let author = v["author"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a["literal"].as_str().or(a["family"].as_str()))
            .or(v["container-title"].as_str())
            .unwrap_or("Unknown");

        let year = crate::export::date_parts(&v, "issued")
            .map(|d| d.year.to_string())
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

/// The distinct entries a batch cites, and the entry each position cites.
///
/// Identical items are one entry: the same item cited twice is one
/// bibliography entry, and hayagriva tells entries apart by address when it
/// numbers and disambiguates citations — a second copy was numbered "[3]"
/// against its own entry's "[1]", and an id-less copy was disambiguated
/// against itself ("2024a"/"2024b"). Every entry gets an id no other entry
/// has, since the bibliography is looked up by id: distinct items sharing
/// one all rendered the first item's reference. Numeric ids are valid
/// CSL-JSON and keep their identity, as strings.
fn batch_entries(items: Vec<serde_json::Value>) -> (Vec<serde_json::Value>, Vec<usize>) {
    fn own_id(item: &serde_json::Value) -> Option<String> {
        crate::export::text_field(item, "id").filter(|id| !id.is_empty())
    }

    // Synthetic ids must not take an id an item brings, even a later one.
    let brought: HashSet<String> = items.iter().filter_map(own_id).collect();
    let mut assigned: HashSet<String> = HashSet::new();
    // `serde_json::Map` is ordered by key, so equal items serialize equally.
    let mut by_content: HashMap<String, usize> = HashMap::new();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut cited = Vec::with_capacity(items.len());

    for (i, mut item) in items.into_iter().enumerate() {
        let content = item.to_string();
        if let Some(&entry) = by_content.get(&content) {
            cited.push(entry);
            continue;
        }
        let id = match own_id(&item) {
            Some(id) if assigned.insert(id.clone()) => id,
            _ => (0..)
                .map(|n| match n {
                    0 => format!("_citeme_batch_{i}"),
                    n => format!("_citeme_batch_{i}_{n}"),
                })
                .find(|id| !brought.contains(id) && assigned.insert(id.clone()))
                .unwrap_or_default(),
        };
        if let Some(fields) = item.as_object_mut() {
            fields.insert("id".into(), serde_json::Value::String(id));
        }
        by_content.insert(content, entries.len());
        cited.push(entries.len());
        entries.push(item);
    }
    (entries, cited)
}

/// The error for an item `csl_item::prepare` refused; batch items are
/// named by position.
fn item_error(problem: ItemProblem, index: Option<usize>) -> EngineError {
    let at = |message: String| match index {
        Some(i) => format!("item {i}: {message}"),
        None => message,
    };
    match problem {
        ItemProblem::Invalid(message) => EngineError::InvalidCslJson(at(message)),
        ItemProblem::Unsupported(message) => EngineError::UnsupportedCslJson(at(message)),
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
        assert!(!engine.has_locale("en-US"));

        engine.load_style("apa", SAMPLE_CSL).unwrap();
        assert!(engine.has_style("apa"));

        // Loading same style again is a no-op
        engine.load_style("apa", SAMPLE_CSL).unwrap();

        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();
        assert!(engine.has_locale("en-US"));
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

        assert!(
            result.reference.contains("Smith"),
            "reference should contain author: {}",
            result.reference
        );
        assert!(
            result.reference.contains("2024"),
            "reference should contain year: {}",
            result.reference
        );
        assert!(
            result.reference.contains("A Study of Something"),
            "reference should contain title: {}",
            result.reference
        );
        // Hayagriva uses <span style="font-style: italic;"> rather than <i>
        assert!(
            result.reference.contains("font-style: italic"),
            "APA reference should contain italics (HTML mode): {}",
            result.reference
        );
        assert!(
            !result.reference.is_empty(),
            "reference should not be empty"
        );

        assert!(
            result.in_text.contains("Smith"),
            "in_text should contain author: {}",
            result.in_text
        );
        assert!(
            result.in_text.contains("2024"),
            "in_text should contain year: {}",
            result.in_text
        );
        assert!(
            result.in_text.contains('(') && result.in_text.contains(')'),
            "APA in_text should have parentheses: {}",
            result.in_text
        );
    }

    #[test]
    fn test_format_one_style_not_loaded() {
        let engine = CitationEngine::new();
        let result = engine.format_one("{}", "nonexistent", "en-US", &FormatOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_format_one_unloaded_locale_errors() {
        // Formatting with a locale code that was never loaded used to render
        // silently with NO locale terms at all — numeric months, empty `and`
        // term ("Brevet déposé le 3 2 2021publié…"), the exact output that
        // failed CiteMe's iso690-fr parity gate. It must be a loud error.
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json = r#"{"type":"book","title":"T","issued":{"date-parts":[[2024]]}}"#;
        let result = engine.format_one(csl_json, "apa", "fr-FR", &FormatOptions::default());
        assert!(
            matches!(result, Err(EngineError::LocaleNotLoaded(ref code)) if code == "fr-FR"),
            "expected LocaleNotLoaded(fr-FR), got: {result:?}"
        );

        // The batch path takes the same guard — even for an empty batch.
        let result = engine.format_batch("[]", "apa", "fr-FR", &FormatOptions::default());
        assert!(
            matches!(result, Err(EngineError::LocaleNotLoaded(_))),
            "batch must take the same guard, got: {result:?}"
        );

        // Empty code stays the explicit escape hatch: "let the style's
        // default-locale pick among the loaded files".
        let result = engine.format_one(csl_json, "apa", "", &FormatOptions::default());
        assert!(
            result.is_ok(),
            "empty locale code must remain permitted: {result:?}"
        );
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

        let result = engine
            .format_one(csl_json, "apa", "en-US", &FormatOptions::default())
            .unwrap();
        assert!(
            !result.reference.is_empty(),
            "reference must never be empty"
        );
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
        let results = engine
            .format_batch(csl_json_array, "apa", "en-US", &opts)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].reference.contains("Smith"));
        assert!(results[1].reference.contains("Doe"));
    }

    #[test]
    fn test_format_batch_empty() {
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let results = engine
            .format_batch("[]", "apa", "en-US", &FormatOptions::default())
            .unwrap();
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
        assert!(
            result.reference.contains("SILVA"),
            "ABNT reference should have uppercased family name: {}",
            result.reference
        );
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
        let results = engine
            .format_batch(csl_json_array, "apa", "en-US", &opts)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert!(
            !results[0].reference.is_empty(),
            "first item reference should not be empty: {:?}",
            results[0]
        );
        assert!(
            !results[1].reference.is_empty(),
            "second item reference should not be empty: {:?}",
            results[1]
        );
        assert!(
            results[0].reference.contains("Smith"),
            "first reference should contain Smith: {}",
            results[0].reference
        );
        assert!(
            results[1].reference.contains("Doe"),
            "second reference should contain Doe: {}",
            results[1].reference
        );
    }

    #[test]
    fn test_format_batch_duplicate_ids_distinct_items() {
        // Two DISTINCT items sharing an id must not collide in the
        // bibliography key lookup — before the fix, both results carried
        // the first item's reference.
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json_array = r#"[
            {
                "type": "article-journal",
                "id": "shared",
                "title": "First Paper",
                "author": [{"family": "Smith", "given": "John"}],
                "issued": {"date-parts": [[2024]]}
            },
            {
                "type": "book",
                "id": "shared",
                "title": "A Great Book",
                "author": [{"family": "Doe", "given": "Jane"}],
                "issued": {"date-parts": [[2023]]}
            }
        ]"#;

        let results = engine
            .format_batch(csl_json_array, "apa", "en-US", &FormatOptions::default())
            .unwrap();

        assert_eq!(results.len(), 2);
        assert!(
            results[0].reference.contains("Smith"),
            "first result must render the first item: {}",
            results[0].reference
        );
        assert!(
            results[1].reference.contains("Doe"),
            "second result must render the second item, not the first: {}",
            results[1].reference
        );
        assert!(
            !results[1].reference.contains("Smith"),
            "second result must not carry the first item's reference: {}",
            results[1].reference
        );
    }

    #[test]
    fn test_format_batch_duplicate_ids_identical_items() {
        // The SAME item cited twice under one id is one bibliography entry,
        // not an ambiguity — both results must match and no year-suffix
        // disambiguation ("2024a") may appear.
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let item = r#"{
                "type": "article-journal",
                "id": "same",
                "title": "First Paper",
                "author": [{"family": "Smith", "given": "John"}],
                "issued": {"date-parts": [[2024]]}
            }"#;
        let csl_json_array = format!("[{item},{item}]");

        let results = engine
            .format_batch(&csl_json_array, "apa", "en-US", &FormatOptions::default())
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].reference, results[1].reference,
            "identical duplicates must render identically"
        );
        assert!(
            !results[0].reference.contains("2024a"),
            "identical duplicates must not trigger year-suffix disambiguation: {}",
            results[0].reference
        );
    }

    #[test]
    fn test_format_batch_numeric_ids_keep_identity() {
        // CSL-JSON allows numeric ids. They were treated as missing and
        // replaced with per-position synthetic ids, so the same item cited
        // twice became two entries and APA disambiguated it against itself
        // ("2024a"/"2024b").
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let item = r#"{"id": 7, "type": "book", "title": "Same",
            "author": [{"family": "Smith", "given": "J"}],
            "issued": {"date-parts": [[2024]]}}"#;
        let results = engine
            .format_batch(
                &format!("[{item},{item}]"),
                "apa",
                "en-US",
                &FormatOptions::default(),
            )
            .unwrap();
        assert_eq!(results[0].reference, results[1].reference);
        assert!(
            !results[0].reference.contains("2024a"),
            "one item cited twice must not be disambiguated: {}",
            results[0].reference
        );
    }

    fn ieee_engine() -> CitationEngine {
        let mut engine = CitationEngine::new();
        engine
            .load_style(
                "ieee",
                include_str!("../../../tests/fixtures/styles/ieee.csl"),
            )
            .unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();
        engine
    }

    #[test]
    fn test_format_batch_numbers_an_item_cited_twice_once() {
        // hayagriva tells entries apart by address when it numbers and
        // disambiguates citations: the same item cited again from a second
        // copy got "[3]" in a numeric style while its entry said "[1]".
        let alpha = r#"{"id": "a", "type": "article-journal", "title": "Alpha",
            "author": [{"family": "Smith", "given": "John"}], "issued": {"date-parts": [[2020]]}}"#;
        let beta = r#"{"id": "b", "type": "article-journal", "title": "Beta",
            "author": [{"family": "Smith", "given": "John"}], "issued": {"date-parts": [[2020]]}}"#;
        let batch = format!("[{alpha},{beta},{alpha}]");

        let ieee = ieee_engine()
            .format_batch(&batch, "ieee", "en-US", &FormatOptions::default())
            .unwrap();
        assert_eq!(ieee[0].in_text, "[1]");
        assert_eq!(ieee[2].in_text, "[1]", "{ieee:?}");
        assert_eq!(ieee[2].reference, ieee[0].reference);

        let mut apa = CitationEngine::new();
        apa.load_style("apa", SAMPLE_CSL).unwrap();
        apa.load_locale("en-US", SAMPLE_LOCALE).unwrap();
        let apa = apa
            .format_batch(&batch, "apa", "en-US", &FormatOptions::default())
            .unwrap();
        assert_eq!(apa[0].in_text, "(Smith, 2020a)");
        assert_eq!(apa[2].in_text, apa[0].in_text, "{apa:?}");
    }

    #[test]
    fn test_format_batch_identical_items_without_id_are_one_entry() {
        // Without an id each copy got its own synthetic id, so the same item
        // was disambiguated against itself ("2024a"/"2024b").
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();
        let item = r#"{"type": "book", "title": "Same",
            "author": [{"family": "Smith", "given": "J"}], "issued": {"date-parts": [[2024]]}}"#;
        let results = engine
            .format_batch(
                &format!("[{item},{item}]"),
                "apa",
                "en-US",
                &FormatOptions::default(),
            )
            .unwrap();
        assert_eq!(results[0].reference, results[1].reference);
        assert!(!results[0].reference.contains("2024a"), "{results:?}");
    }

    #[test]
    fn test_format_batch_synthetic_ids_never_take_an_item_id() {
        // The id-less item's synthetic id was "_citeme_batch_1" — the id the
        // first item already carried — and it rendered the first item.
        let mut engine = CitationEngine::new();
        engine.load_style("apa", SAMPLE_CSL).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();
        let batch = r#"[
            {"id": "_citeme_batch_1", "type": "book", "title": "First",
             "author": [{"family": "Smith", "given": "J"}], "issued": {"date-parts": [[2020]]}},
            {"type": "book", "title": "Second",
             "author": [{"family": "Doe", "given": "J"}], "issued": {"date-parts": [[2021]]}}
        ]"#;
        let results = engine
            .format_batch(batch, "apa", "en-US", &FormatOptions::default())
            .unwrap();
        assert!(results[1].reference.contains("Doe"), "{results:?}");
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
        let result = engine
            .format_one(csl_json, "apa", "en-US", &prose_opts)
            .unwrap();

        // Prose citation: "Smith (2024)" — author outside parens, year inside
        assert!(
            !result.in_text.is_empty(),
            "prose in_text must not be empty"
        );
        assert!(
            result.in_text.contains("Smith"),
            "prose in_text should contain author: {}",
            result.in_text
        );
        assert!(
            result.in_text.contains("2024"),
            "prose in_text should contain year: {}",
            result.in_text
        );
        // Positive: prose form starts with bare author name
        let stripped = result.in_text.replace("<span", "").replace("</span>", "");
        assert!(
            !stripped.trim_start().starts_with('('),
            "prose in_text should start with bare author, not '(' — got: {}",
            result.in_text
        );
    }

    #[test]
    fn test_date_delimiter_no_trailing_space_artifact() {
        // Regression: Hayagriva renders `<date delimiter=" ">` between every
        // date-part even when neighbors are empty, producing "2018 ;" for
        // year-only dates when the parent group adds a suffix. The CSL-XML
        // normalizer rewrites the delimiter to per-part `prefix=" "`, which
        // Hayagriva correctly suppresses on empty-preceding-siblings.
        let style = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">
  <info><title>t</title><id>t</id><updated>2024-01-01T00:00:00+00:00</updated></info>
  <locale xml:lang="en">
    <date form="text" delimiter=" ">
      <date-part name="year"/>
      <date-part name="month" form="short" strip-periods="true"/>
      <date-part name="day"/>
    </date>
  </locale>
  <citation><layout><text value="x"/></layout></citation>
  <bibliography>
    <layout>
      <group suffix=";">
        <date variable="issued" form="text"/>
      </group>
    </layout>
  </bibliography>
</style>"#;

        let mut engine = CitationEngine::new();
        engine.load_style("t", style).unwrap();
        engine.load_locale("en-US", SAMPLE_LOCALE).unwrap();

        let csl_json = r#"{"id":"x","type":"article-journal","issued":{"date-parts":[[2018]]}}"#;
        let opts = FormatOptions {
            output_format: OutputFormat::Plain,
            ..Default::default()
        };
        let out = engine.format_one(csl_json, "t", "en-US", &opts).unwrap();
        assert!(
            !out.reference.contains(" ;"),
            "year-only date must not leak a trailing-space artifact before the group suffix; got: {:?}",
            out.reference
        );
        assert!(
            out.reference.contains("2018;"),
            "expected '2018;' (year+suffix glued) in: {:?}",
            out.reference
        );
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
        let results = engine
            .format_batch(csl_json_array, "apa", "en-US", &prose_opts)
            .unwrap();

        assert_eq!(results.len(), 2);
        // Both must be non-empty
        assert!(
            !results[0].in_text.is_empty(),
            "first prose citation must not be empty"
        );
        assert!(
            !results[1].in_text.is_empty(),
            "second prose citation must not be empty"
        );
        // Positive: contains author names
        assert!(
            results[0].in_text.contains("Smith"),
            "first prose citation should contain Smith: {}",
            results[0].in_text
        );
        assert!(
            results[1].in_text.contains("Doe"),
            "second prose citation should contain Doe: {}",
            results[1].in_text
        );
        // Negative: should not be parenthetical form
        let s0 = results[0]
            .in_text
            .replace("<span", "")
            .replace("</span>", "");
        let s1 = results[1]
            .in_text
            .replace("<span", "")
            .replace("</span>", "");
        assert!(
            !s0.trim_start().starts_with('('),
            "first prose citation should not be parenthetical: {}",
            results[0].in_text
        );
        assert!(
            !s1.trim_start().starts_with('('),
            "second prose citation should not be parenthetical: {}",
            results[1].in_text
        );
    }
}
