use std::collections::HashMap;

use hayagriva::citationberg::{IndependentStyle, LocaleFile, Locale, Style};

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
}
