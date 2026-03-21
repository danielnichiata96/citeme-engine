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
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmCitationEngine {
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

    /// Get engine version.
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}
