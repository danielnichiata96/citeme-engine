//! What `format_one` / `format_batch` do with CSL-JSON hayagriva can't read
//! as-is.
//!
//! hayagriva reads an item through `citationberg::json::Item`, whose values
//! must each be a string, an integer, a name list or a date. Two valid date
//! shapes then reach a panic — an abort that poisons the Wasm instance — and
//! anything else (null, a boolean, a float, a keyword array, an extension
//! object) failed the whole item. That last case included the item CiteMe
//! builds for every arXiv paper, so none of them ever formatted.

use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::error::EngineError;
use citeme_engine_core::types::{FormatOptions, FormatResult, OutputFormat};
use serde_json::{json, Value};

fn engine() -> CitationEngine {
    let mut engine = CitationEngine::new();
    engine
        .load_style(
            "apa",
            include_str!("../../../tests/fixtures/styles/apa.csl"),
        )
        .unwrap();
    engine
        .load_locale(
            "en-US",
            include_str!("../../../tests/fixtures/locales/locales-en-US.xml"),
        )
        .unwrap();
    engine
}

fn plain() -> FormatOptions {
    FormatOptions {
        output_format: OutputFormat::Plain,
        ..Default::default()
    }
}

fn format(item: &Value) -> Result<FormatResult, EngineError> {
    engine().format_one(&item.to_string(), "apa", "en-US", &plain())
}

fn webpage_issued(date: Value) -> Value {
    json!({
        "id": "w", "type": "webpage", "title": "Page",
        "author": [{"family": "Doe", "given": "Jane"}],
        "issued": date, "URL": "https://example.org"
    })
}

/// The item CiteMe's `unifiedPaperToCsl` emits for an arXiv paper: CSL has
/// no arXiv variable, so the id travels in a `custom` extension object.
fn citeme_arxiv_paper(id: &str) -> Value {
    json!({
        "type": "article-journal", "id": id,
        "title": "Attention Is All You Need",
        "author": [{"family": "Vaswani", "given": "Ashish"}],
        "issued": {"date-parts": [[2017]]},
        "container-title": "arXiv",
        "custom": {"eprint": {"id": "1706.03762", "type": "arxiv"}}
    })
}

#[test]
fn a_date_range_is_a_clean_error_not_a_panic() {
    for date in [
        json!({"date-parts": [[2019, 5], [2020, 6]]}),
        json!({"raw": "2019/2020"}),
    ] {
        let result = format(&webpage_issued(date.clone()));
        assert!(
            matches!(result, Err(EngineError::UnsupportedCslJson(ref m)) if m.contains("issued")),
            "{date}: {result:?}"
        );
    }
}

#[test]
fn a_batch_with_a_date_range_is_a_clean_error_not_a_panic() {
    let items = json!([
        webpage_issued(json!({"date-parts": [[2019]]})),
        {"type": "book", "title": "T", "event-date": {"date-parts": [[2019, 6, 1], [2019, 6, 5]]}}
    ]);
    let result = engine().format_batch(&items.to_string(), "apa", "en-US", &plain());
    assert!(
        matches!(result, Err(EngineError::UnsupportedCslJson(ref m)) if m.contains("event-date")),
        "{result:?}"
    );
}

#[test]
fn a_range_that_ends_where_it_starts_is_one_date() {
    let single = format(&webpage_issued(json!({"date-parts": [[2019, 5, 3]]}))).unwrap();
    for date in [
        json!({"date-parts": [[2019, 5, 3], [2019, 5, 3]]}),
        json!({"date-parts": [[2019, 5, 3], []]}),
        json!({"raw": "2019-05-03/2019-05-03"}),
    ] {
        let result = format(&webpage_issued(date.clone())).unwrap();
        assert_eq!(result.reference, single.reference, "{date}");
    }
}

#[test]
fn an_empty_date_is_no_date() {
    let undated = format(&webpage_issued(Value::Null)).unwrap();
    assert!(undated.reference.contains("n.d."), "{}", undated.reference);
    for date in [
        json!({"date-parts": [[]]}),
        json!({"date-parts": [[""]]}),
        json!({"date-parts": []}),
        json!({}),
    ] {
        let result = format(&webpage_issued(date.clone())).unwrap();
        assert_eq!(result.reference, undated.reference, "{date}");
    }
}

#[test]
fn out_of_range_months_and_days_are_dropped_not_printed() {
    // hayagriva printed month 0 as "(2019, 256 3)" and day 40 as "May 40".
    let year = format(&webpage_issued(json!({"date-parts": [[2019]]}))).unwrap();
    let month = format(&webpage_issued(json!({"date-parts": [[2019, 5]]}))).unwrap();
    for (date, expected) in [
        (json!({"date-parts": [[2019, 0, 3]]}), &year),
        (json!({"date-parts": [[2019, 13]]}), &year),
        (json!({"date-parts": [["2019", "-1"]]}), &year),
        (json!({"date-parts": [[2019, 5, 40]]}), &month),
        (json!({"date-parts": [[2019, 5, 0]]}), &month),
        (json!({"raw": "2019-00-03"}), &year),
    ] {
        let result = format(&webpage_issued(date.clone())).unwrap();
        assert_eq!(result.reference, expected.reference, "{date}");
    }
}

#[test]
fn fields_hayagriva_cannot_hold_are_ignored() {
    let clean = citeme_arxiv_paper("p1");
    let mut noisy = clean.clone();
    let obj = noisy.as_object_mut().unwrap();
    obj.insert("note".into(), Value::Null);
    obj.insert("keyword".into(), json!(["transformers", "attention"]));
    obj.insert("suppress-author".into(), json!(true));
    obj.insert("relations".into(), json!({"isPartOf": ["x"]}));
    obj.insert(
        "author".into(),
        json!([{"family": "Vaswani", "given": "Ashish", "sequence": "first", "affiliation": []}]),
    );

    let without_custom = {
        let mut v = clean.clone();
        v.as_object_mut().unwrap().remove("custom");
        format(&v).unwrap()
    };
    assert_eq!(format(&clean).unwrap().reference, without_custom.reference);
    assert_eq!(format(&noisy).unwrap().reference, without_custom.reference);
}

#[test]
fn citeme_arxiv_papers_format_in_a_batch() {
    let items = json!([citeme_arxiv_paper("a"), citeme_arxiv_paper("b")]);
    let results = engine()
        .format_batch(&items.to_string(), "apa", "en-US", &plain())
        .unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0].reference.contains("Vaswani"), "{results:?}");
}

#[test]
fn numbers_json_cannot_hold_as_integers_are_read_as_text() {
    let mut item = webpage_issued(json!({"date-parts": [[2019]]}));
    item["type"] = json!("article-journal");
    item["container-title"] = json!("Journal");
    item["volume"] = json!(1.5);
    item["issue"] = json!(18446744073709551615u64);
    let result = format(&item).unwrap();
    assert!(result.reference.contains("1.5"), "{}", result.reference);
    assert!(
        result.reference.contains("18446744073709551615"),
        "{}",
        result.reference
    );
}

#[test]
fn a_literal_only_date_is_a_clean_error() {
    // hayagriva has no use for a date's `literal`; silently printing "n.d."
    // instead would hide that the item's date was dropped.
    let result = format(&webpage_issued(json!({"literal": "Spring 2019"})));
    assert!(
        matches!(result, Err(EngineError::UnsupportedCslJson(ref m)) if m.contains("issued")),
        "{result:?}"
    );
}

#[test]
fn a_name_list_that_is_not_names_is_invalid() {
    let mut item = webpage_issued(json!({"date-parts": [[2019]]}));
    item["author"] = json!("Doe, Jane");
    let result = format(&item);
    assert!(
        matches!(result, Err(EngineError::InvalidCslJson(ref m)) if m.contains("author")),
        "{result:?}"
    );
}

fn iso690_fr(item: &Value) -> Result<FormatResult, EngineError> {
    let mut engine = CitationEngine::new();
    engine
        .load_style(
            "iso690-fr",
            include_str!("../../../tests/fixtures/styles/iso690-author-date-fr.csl"),
        )
        .unwrap();
    engine
        .load_locale(
            "fr-FR",
            include_str!("../../../tests/fixtures/locales/locales-fr-FR.xml"),
        )
        .unwrap();
    engine.format_one(&item.to_string(), "iso690-fr", "fr-FR", &plain())
}

/// `volume` and `page` both set to `numbers`, or both absent.
fn french_article(numbers: Option<&str>) -> Value {
    let mut item = json!({
        "id": "x", "type": "article-journal", "title": "Titre",
        "author": [{"family": "Dupont", "given": "Marie"}],
        "container-title": "Revue", "issued": {"date-parts": [[2020]]}
    });
    if let Some(numbers) = numbers {
        item["volume"] = json!(numbers);
        item["page"] = json!(numbers);
    }
    item
}

#[test]
fn surrounding_whitespace_is_trimmed_not_a_panic() {
    // The style prints `volume` after "Vol.&#160;". A value opening with a
    // space makes hayagriva trim that NBSP, then slice the text at the
    // prefix's old byte length — inside "é": a panic.
    let trimmed = iso690_fr(&french_article(Some("é"))).unwrap();
    for volume in [" é", "é  ", "\u{a0}é\n"] {
        let result = iso690_fr(&french_article(Some(volume)));
        assert_eq!(result.unwrap().reference, trimmed.reference, "{volume:?}");
    }
}

#[test]
fn a_whitespace_only_value_is_an_absent_value() {
    // " " printed a dangling "2020., ." where volume and pages would be.
    let without = iso690_fr(&french_article(None)).unwrap();
    for volume in [" ", "\u{a0}", "\t\n"] {
        let result = iso690_fr(&french_article(Some(volume)));
        assert_eq!(result.unwrap().reference, without.reference, "{volume:?}");
    }
}

/// `(reference, in_text)` through CiteMe's ABNT 2023 style.
fn abnt_2023(item: &Value, post_process: bool) -> (String, String) {
    let mut engine = CitationEngine::new();
    engine
        .load_style(
            "abnt",
            include_str!("../../../tests/fixtures/styles/corpus/abnt-2023.csl"),
        )
        .unwrap();
    engine
        .load_locale(
            "pt-BR",
            include_str!("../../../tests/fixtures/locales/locales-pt-BR.xml"),
        )
        .unwrap();
    let options = FormatOptions {
        abnt_post_process: post_process,
        ..plain()
    };
    let result = engine
        .format_one(&item.to_string(), "abnt", "pt-BR", &options)
        .unwrap();
    (result.reference, result.in_text)
}

#[test]
fn abnt_post_processing_never_uppercases_a_title() {
    // With no author, editor or translator the ABNT styles lead with the
    // title. Post-processing took the text before the first comma for a
    // family name: "PESQUISA NACIONAL POR AMOSTRA DE DOMICÍLIOS, síntese
    // de indicadores", and the in-text "(MANUAL DE REDAÇÃO, 2010)".
    for title in [
        "Pesquisa nacional por amostra de domicílios, síntese de indicadores",
        "Manual de redação",
    ] {
        let item = json!({"type": "book", "title": title, "publisher": "IBGE",
            "issued": {"date-parts": [[2015]]}});
        assert_eq!(abnt_2023(&item, true), abnt_2023(&item, false), "{title}");
    }
}

#[test]
fn abnt_post_processing_still_uppercases_names() {
    let item = json!({"type": "book", "title": "Livro", "publisher": "Ed",
        "author": [{"family": "Silva", "given": "João"}, {"family": "Souza", "given": "Ana"},
                   {"family": "Lima", "given": "Rui"}, {"family": "Costa", "given": "Eva"}],
        "issued": {"date-parts": [[2024]]}});
    let (reference, in_text) = abnt_2023(&item, true);
    assert_eq!(in_text, "(SILVA et al., 2024)");
    assert!(reference.starts_with("SILVA, João"), "{reference}");

    let edited = json!({"type": "book", "title": "Livro", "publisher": "Ed",
        "editor": [{"family": "Souza", "given": "Ana"}], "issued": {"date-parts": [[2024]]}});
    assert_eq!(abnt_2023(&edited, true).1, "(SOUZA, 2024)");
}

#[test]
fn a_value_opening_with_punctuation_keeps_its_value() {
    // The style prints `volume` and `page` after "Vol.&#160;"/"pp.&#160;".
    // A value opening with punctuation makes hayagriva trim that NBSP
    // ("punctuation eats spaces"), but it measured the prefix before the
    // trim: the offset fell inside "é" — a panic — or past a short value,
    // which then read as empty and was dropped ("2020., ."). Fixed in the
    // vendored hayagriva (vendor/README.md).
    for value in [".e", ".é", ",é", ")é", "]é", ".\u{1e006}:"] {
        let result = iso690_fr(&french_article(Some(value)));
        let reference = result.unwrap().reference;
        assert!(
            reference.contains(&format!("Vol.{value}"))
                && reference.contains(&format!("pp.{value}")),
            "{value:?}: {reference}"
        );
    }
}
