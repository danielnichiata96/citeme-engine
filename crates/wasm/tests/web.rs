use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use citeme_engine_wasm::WasmCitationEngine;

#[wasm_bindgen_test]
fn test_wasm_format_one_apa() {
    let mut engine = WasmCitationEngine::new();

    let csl = include_str!("../../../tests/fixtures/styles/apa.csl");
    let locale = include_str!("../../../tests/fixtures/locales/locales-en-US.xml");

    engine.load_style("apa", csl).unwrap();
    engine.load_locale("en-US", locale).unwrap();

    let input = r#"{"type":"article-journal","title":"Test","author":[{"family":"Smith","given":"John"}],"issued":{"date-parts":[[2024]]}}"#;

    let result_str = engine.format_one(input, "apa", "en-US", false).unwrap();
    let result: serde_json::Value = serde_json::from_str(&result_str).unwrap();

    assert!(result["reference"].as_str().unwrap().contains("Smith"));
    assert!(result["inText"].as_str().unwrap().contains("2024"));
}

#[wasm_bindgen_test]
fn test_wasm_version() {
    let engine = WasmCitationEngine::new();
    assert_eq!(engine.version(), "0.1.0");
}
