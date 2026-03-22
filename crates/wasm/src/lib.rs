use wasm_bindgen::prelude::*;
use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::types::{FormatOptions, OutputFormat};

/// The main engine struct exposed to JavaScript.
/// Holds compiled CSL styles and locales in Wasm linear memory.
#[wasm_bindgen]
pub struct WasmCitationEngine {
    inner: CitationEngine,
}

#[wasm_bindgen]
impl WasmCitationEngine {
    /// Create a new engine instance.
    /// Installs panic hook so Rust panics produce readable JS console errors.
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmCitationEngine {
        console_error_panic_hook::set_once();
        WasmCitationEngine {
            inner: CitationEngine::new(),
        }
    }

    /// Load a CSL style from XML string. Compiled and cached.
    #[wasm_bindgen(js_name = "loadStyle")]
    pub fn load_style(&mut self, name: &str, csl_xml: &str) -> Result<(), JsError> {
        self.inner.load_style(name, csl_xml)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Load a locale from XML string.
    #[wasm_bindgen(js_name = "loadLocale")]
    pub fn load_locale(&mut self, locale_code: &str, locale_xml: &str) -> Result<(), JsError> {
        self.inner.load_locale(locale_code, locale_xml)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Check if a style is loaded.
    #[wasm_bindgen(js_name = "hasStyle")]
    pub fn has_style(&self, name: &str) -> bool {
        self.inner.has_style(name)
    }

    /// Format a batch of CSL-JSON items.
    /// Input: JSON string of CSL-JSON array. Output: JSON string of FormatResult array.
    #[wasm_bindgen(js_name = "formatBatch")]
    pub fn format_batch(
        &self,
        csl_json_str: &str,
        style_name: &str,
        locale_code: &str,
        abnt_post_process: bool,
    ) -> Result<String, JsError> {
        let options = FormatOptions {
            output_format: OutputFormat::Html,
            abnt_post_process,
        };

        let results = self.inner.format_batch(csl_json_str, style_name, locale_code, &options)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&results)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Format a single CSL-JSON item.
    /// Input: JSON string of one CSL-JSON item. Output: JSON string of FormatResult.
    #[wasm_bindgen(js_name = "formatOne")]
    pub fn format_one(
        &self,
        csl_json_str: &str,
        style_name: &str,
        locale_code: &str,
        abnt_post_process: bool,
    ) -> Result<String, JsError> {
        let options = FormatOptions {
            output_format: OutputFormat::Html,
            abnt_post_process,
        };

        let result = self.inner.format_one(csl_json_str, style_name, locale_code, &options)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&result)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Parse BibTeX content and return CSL-JSON array.
    #[wasm_bindgen(js_name = "parseBibtex")]
    pub fn parse_bibtex(&self, input: &str, max_entries: Option<usize>) -> Result<String, JsError> {
        let options = citeme_engine_core::parsers::ParseOptions {
            max_entries,
            ..Default::default()
        };
        let result = citeme_engine_core::parsers::bibtex::parse_bibtex(input, &options);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Parse RIS content and return CSL-JSON array.
    #[wasm_bindgen(js_name = "parseRis")]
    pub fn parse_ris(&self, input: &str, max_entries: Option<usize>) -> Result<String, JsError> {
        let options = citeme_engine_core::parsers::ParseOptions {
            max_entries,
            ..Default::default()
        };
        let result = citeme_engine_core::parsers::ris::parse_ris(input, &options);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Detect input format (returns "bibtex", "ris", "csl-json", or "unknown").
    #[wasm_bindgen(js_name = "detectFormat")]
    pub fn detect_format(&self, input: &str) -> String {
        citeme_engine_core::parsers::detect::detect_format(input).as_str().to_string()
    }

    /// Auto-detect format and parse. Returns JSON string of ParseResult.
    #[wasm_bindgen(js_name = "parseAuto")]
    pub fn parse_auto(&self, input: &str, max_entries: Option<usize>) -> Result<String, JsError> {
        use citeme_engine_core::parsers::detect::InputFormat;
        let format = citeme_engine_core::parsers::detect::detect_format(input);
        let options = citeme_engine_core::parsers::ParseOptions {
            max_entries,
            ..Default::default()
        };
        let result = match format {
            InputFormat::Bibtex => citeme_engine_core::parsers::bibtex::parse_bibtex(input, &options),
            InputFormat::Ris => citeme_engine_core::parsers::ris::parse_ris(input, &options),
            InputFormat::CslJson => citeme_engine_core::parsers::csl_json::parse_csl_json(input, &options),
            InputFormat::Medline => citeme_engine_core::parsers::medline::parse_medline(input, &options),
            InputFormat::Unknown => citeme_engine_core::parsers::ParseResult {
                entries: vec![],
                errors: vec![citeme_engine_core::parsers::ParseErrorInfo {
                    preview: input.chars().take(80).collect(),
                    error: "could not detect format".to_string(),
                }],
                format: "unknown".to_string(),
                truncated: false,
                scanned_entries: 0,
            },
        };
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Validate and pass through CSL-JSON input. Accepts single object or array.
    #[wasm_bindgen(js_name = "parseCslJson")]
    pub fn parse_csl_json(&self, input: &str, max_entries: Option<usize>) -> Result<String, JsError> {
        let options = citeme_engine_core::parsers::ParseOptions {
            max_entries,
            ..Default::default()
        };
        let result = citeme_engine_core::parsers::csl_json::parse_csl_json(input, &options);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Parse MEDLINE/NBIB content and return CSL-JSON array.
    #[wasm_bindgen(js_name = "parseMedline")]
    pub fn parse_medline(&self, input: &str, max_entries: Option<usize>) -> Result<String, JsError> {
        let options = citeme_engine_core::parsers::ParseOptions {
            max_entries,
            ..Default::default()
        };
        let result = citeme_engine_core::parsers::medline::parse_medline(input, &options);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Export CSL-JSON to BibTeX string. Input: JSON string (object or array).
    #[wasm_bindgen(js_name = "exportBibtex")]
    pub fn export_bibtex(&self, csl_json_str: &str) -> Result<String, JsError> {
        let value: serde_json::Value = serde_json::from_str(csl_json_str)
            .map_err(|e| JsError::new(&e.to_string()))?;
        match value {
            serde_json::Value::Array(items) => Ok(citeme_engine_core::export::bibtex::csl_json_array_to_bibtex(&items)),
            obj @ serde_json::Value::Object(_) => Ok(citeme_engine_core::export::bibtex::csl_json_to_bibtex(&obj)),
            _ => Err(JsError::new("expected JSON object or array")),
        }
    }

    /// Export CSL-JSON to RIS string. Input: JSON string (object or array).
    #[wasm_bindgen(js_name = "exportRis")]
    pub fn export_ris(&self, csl_json_str: &str) -> Result<String, JsError> {
        let value: serde_json::Value = serde_json::from_str(csl_json_str)
            .map_err(|e| JsError::new(&e.to_string()))?;
        match value {
            serde_json::Value::Array(items) => Ok(citeme_engine_core::export::ris::csl_json_array_to_ris(&items)),
            obj @ serde_json::Value::Object(_) => Ok(citeme_engine_core::export::ris::csl_json_to_ris(&obj)),
            _ => Err(JsError::new("expected JSON object or array")),
        }
    }

    /// Export CSL-JSON to Hayagriva YAML string. Input: JSON string (object or array).
    #[wasm_bindgen(js_name = "exportHayagriva")]
    pub fn export_hayagriva(&self, csl_json_str: &str) -> Result<String, JsError> {
        let value: serde_json::Value = serde_json::from_str(csl_json_str)
            .map_err(|e| JsError::new(&e.to_string()))?;
        match value {
            serde_json::Value::Array(items) => Ok(citeme_engine_core::export::hayagriva::csl_json_array_to_hayagriva(&items)),
            obj @ serde_json::Value::Object(_) => Ok(citeme_engine_core::export::hayagriva::csl_json_to_hayagriva(&obj)),
            _ => Err(JsError::new("expected JSON object or array")),
        }
    }

    /// Get engine version.
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}
