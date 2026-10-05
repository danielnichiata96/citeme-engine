//! Exporter validity: an exported file must be accepted by its own consumer.
//!
//! BibTeX/BibLaTeX output must re-import through the engine's own parser;
//! Hayagriva output must load through hayagriva's YAML reader. This matters
//! because the CiteMe adapter's JS fallback only fires when the Wasm call
//! throws — silently corrupt output sails straight to the user's file, so
//! these tests are the release gate for the export surface.

use citeme_engine_core::export::biblatex::csl_json_array_to_biblatex;
use citeme_engine_core::export::bibtex::{csl_json_array_to_bibtex, csl_json_to_bibtex};
use citeme_engine_core::export::hayagriva::csl_json_array_to_hayagriva;
use citeme_engine_core::export::ris::csl_json_to_ris;
use citeme_engine_core::parsers::bibtex::parse_bibtex;
use citeme_engine_core::parsers::detect::{detect_format, InputFormat};
use citeme_engine_core::parsers::ris::parse_ris;
use citeme_engine_core::parsers::ParseOptions;
use serde_json::{json, Value};

fn reimport(bib: &str) -> (usize, usize) {
    let res = parse_bibtex(bib, &ParseOptions::default());
    (res.entries.len(), res.errors.len())
}

fn hostile_items() -> Vec<Value> {
    vec![
        json!({
            "type": "article-journal",
            "id": "bad,key",
            "title": "Deep {learning for {unbalanced imports",
            "author": [{"family": "Silva", "given": "Ana"}],
            "issued": {"date-parts": [[2024]]}
        }),
        json!({
            "type": "article-journal",
            "id": "ok2024",
            "title": r"The \alpha } factor & a 100% $gain",
            "author": [{"family": "Souza", "given": "Bruno"}],
            "issued": {"date-parts": [[2023]]}
        }),
        json!({
            "type": "book",
            "id": "id:10.1000/x{y},z",
            "title": "Plain title",
            "publisher": "Norm & Co. {Press}",
            "issued": {"date-parts": [[2020]]}
        }),
    ]
}

// ── BibTeX ───────────────────────────────────────────────────────────

#[test]
fn bibtex_export_reimports_comma_id_and_unbalanced_braces() {
    for item in hostile_items() {
        let bib = csl_json_to_bibtex(&item);
        let (entries, errors) = reimport(&bib);
        assert_eq!(entries, 1, "must re-import as exactly one entry:\n{bib}");
        assert_eq!(errors, 0, "must re-import without errors:\n{bib}");
    }
}

#[test]
fn bibtex_array_export_reimports_full_batch() {
    let items = hostile_items();
    let bib = csl_json_array_to_bibtex(&items);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        entries,
        items.len(),
        "batch must survive round-trip:\n{bib}"
    );
    assert_eq!(errors, 0, "batch must re-import cleanly:\n{bib}");
}

#[test]
fn bibtex_cite_key_is_sanitized() {
    let item = json!({
        "type": "book",
        "id": "id:10.1000/a,b {c}(d)=e\\f#g%h\"i~j k",
        "title": "T"
    });
    let bib = csl_json_to_bibtex(&item);
    let first = bib.lines().next().expect("non-empty output");
    let key = first
        .trim_start_matches(|c| c != '{')
        .trim_start_matches('{')
        .trim_end_matches(',');
    assert!(!key.is_empty(), "sanitized key must not be empty: {first}");
    for bad in [',', '{', '}', '(', ')', '=', '\\', '#', '%', '"', '~', ' '] {
        assert!(!key.contains(bad), "key {key:?} contains {bad:?}: {first}");
    }
}

#[test]
fn bibtex_cite_key_falls_back_when_id_fully_invalid() {
    let item = json!({
        "type": "article-journal",
        "id": ",,{}\\ ",
        "title": "T",
        "author": [{"family": "Lima", "given": "Caio"}],
        "issued": {"date-parts": [[2021]]}
    });
    let bib = csl_json_to_bibtex(&item);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        (entries, errors),
        (1, 0),
        "fallback key must keep entry importable:\n{bib}"
    );
}

// ── BibLaTeX ─────────────────────────────────────────────────────────

#[test]
fn biblatex_export_reimports_hostile_batch() {
    let items = hostile_items();
    let bib = csl_json_array_to_biblatex(&items);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        entries,
        items.len(),
        "biblatex batch must survive round-trip:\n{bib}"
    );
    assert_eq!(errors, 0, "biblatex batch must re-import cleanly:\n{bib}");
}

// ── Hayagriva ────────────────────────────────────────────────────────

#[test]
fn hayagriva_yaml_survives_scalar_lookalikes() {
    let items = vec![
        json!({"type": "article-journal", "id": "a1", "title": "true"}),
        json!({"type": "article-journal", "id": "a2", "title": "null"}),
        json!({"type": "article-journal", "id": "a3", "title": "T", "abstract": "- leading dash"}),
        json!({"type": "article-journal", "id": "a4", "title": "@misc looks like a tag"}),
        json!({"type": "article-journal", "id": "a5", "title": "*star: &anchor !tag"}),
        json!({"type": "article-journal", "id": "a6", "title": "T", "volume": "3", "issue": "2-3"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml);
    assert!(
        lib.is_ok(),
        "exported YAML must load: {:?}\n{yaml}",
        lib.err()
    );
    assert_eq!(
        lib.unwrap().len(),
        items.len(),
        "all entries must survive:\n{yaml}"
    );
}

#[test]
fn hayagriva_invalid_language_is_omitted_not_corrupting() {
    // `language` maps to hayagriva's LanguageIdentifier — junk values must be
    // dropped, never emitted (raw OR quoted junk both fail the YAML load).
    let items = vec![
        json!({"type": "article-journal", "id": "l1", "title": "T", "language": "null"}),
        json!({"type": "article-journal", "id": "l2", "title": "T", "language": "en: US"}),
        json!({"type": "article-journal", "id": "l3", "title": "T", "language": "pt-BR"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml);
    assert!(
        lib.is_ok(),
        "junk language must not corrupt YAML: {:?}\n{yaml}",
        lib.err()
    );
    assert!(
        yaml.contains("language: pt-BR"),
        "valid language must be kept:\n{yaml}"
    );
}

// ── Format detection ─────────────────────────────────────────────────

#[test]
fn detect_accepts_any_bibtex_entry_type() {
    assert_eq!(
        detect_format("@dataset{d1, title={T}}"),
        InputFormat::Bibtex
    );
    assert_eq!(
        detect_format("@software{s1, title={T}}"),
        InputFormat::Bibtex
    );
    assert_eq!(
        detect_format("@Collection(c1, title={T})"),
        InputFormat::Bibtex
    );
}

#[test]
fn detect_does_not_false_positive_on_plain_text() {
    assert_eq!(
        detect_format("email me @ home {sometime}"),
        InputFormat::Unknown
    );
    assert_eq!(
        detect_format("mention @user in the thread"),
        InputFormat::Unknown
    );
}

// ── Parse error bounding ─────────────────────────────────────────────

#[test]
fn parse_errors_are_capped() {
    let mut input = String::new();
    for i in 0..300 {
        input.push_str(&format!("@article{{k{i}, title = {{broken\n\n"));
    }
    let res = parse_bibtex(&input, &ParseOptions::default());
    assert!(
        res.errors.len() <= 100,
        "error array must be bounded, got {}",
        res.errors.len()
    );
}

// ── Hostile author names ─────────────────────────────────────────────
//
// The gate above only ever fed hostile *field* values. Author names take a
// different code path (`format_authors`) that skipped escaping entirely, so a
// family name carrying `}` closed the field and injected a second record.

fn hostile_author_items() -> Vec<Value> {
    vec![
        json!({
            "type": "article-journal",
            "id": "inj1",
            "title": "Good title",
            "author": [{"family": "X}, title={pwned}}\n@book{injected", "given": "E"}],
            "issued": {"date-parts": [[2024]]}
        }),
        json!({
            "type": "book",
            "id": "inj2",
            "title": "T",
            "author": [{"literal": "Institute of {Things} & Co. }"}],
            "issued": {"date-parts": [[2020]]}
        }),
        json!({
            "type": "article-journal",
            "id": "inj3",
            "title": "T",
            "editor": [{"family": "A\\b{c}", "given": "D% E$"}],
            "issued": {"date-parts": [[2021]]}
        }),
    ]
}

#[test]
fn bibtex_export_escapes_hostile_author_names() {
    for item in hostile_author_items() {
        let bib = csl_json_to_bibtex(&item);
        // The real test is the round-trip: a `@` inside a properly escaped
        // value is inert, so counting `@` would measure the wrong thing.
        let (entries, errors) = reimport(&bib);
        assert_eq!(
            (entries, errors),
            (1, 0),
            "hostile author must re-import as exactly one clean entry:\n{bib}"
        );
    }
}

#[test]
fn biblatex_export_escapes_hostile_author_names() {
    let items = hostile_author_items();
    let bib = csl_json_array_to_biblatex(&items);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        (entries, errors),
        (items.len(), 0),
        "hostile authors must re-import cleanly:\n{bib}"
    );
}

// ── RIS round-trip ───────────────────────────────────────────────────
//
// RIS is line-oriented: a newline inside any value ends the field, and a
// crafted one ends the whole record and opens another. Nothing was filtered,
// and the re-import reported zero errors — silent corruption, no fallback.

#[test]
fn ris_export_neutralizes_newline_injection() {
    let item = json!({
        "type": "article-journal",
        "id": "r1",
        "title": "Good\nER  - \nTY  - BOOK\nTI  - Injected Book",
        "author": [{"family": "S\nAU  - Ghost, G", "given": "J"}],
        "abstract": "line one\r\nER  - \r\nTY  - CHAP",
        "issued": {"date-parts": [[2024]]}
    });
    let ris = csl_json_to_ris(&item);
    assert_eq!(
        ris.matches("TY  - ").count(),
        1,
        "exactly one record may be opened:\n{ris}"
    );
    assert_eq!(
        ris.matches("ER  - ").count(),
        1,
        "exactly one record may be closed:\n{ris}"
    );
    let back = parse_ris(&ris, &ParseOptions::default());
    assert_eq!(
        back.entries.len(),
        1,
        "must re-import as exactly one entry:\n{ris}"
    );
}

#[test]
fn ris_export_round_trips_core_fields() {
    let item = json!({
        "type": "article-journal",
        "id": "r2",
        "title": "A Study of Things",
        "author": [{"family": "Souza", "given": "Bruno"}],
        "container-title": "Nature",
        "issued": {"date-parts": [[2023]]},
        "DOI": "10.1234/x"
    });
    let ris = csl_json_to_ris(&item);
    let back = parse_ris(&ris, &ParseOptions::default());
    assert_eq!((back.entries.len(), back.errors.len()), (1, 0), "{ris}");
    let got = &back.entries[0];
    assert_eq!(got["title"].as_str(), Some("A Study of Things"), "{ris}");
    assert_eq!(got["DOI"].as_str(), Some("10.1234/x"), "{ris}");
}

// ── Name particles ───────────────────────────────────────────────────
//
// CiteMe emits `non-dropping-particle` for names like "Maria da Silva"
// (paper-to-csl.ts). Every exporter read only `dropping-particle`, so the
// particle was silently dropped — a visible citation error in pt/es/nl/de.

#[test]
fn exports_preserve_non_dropping_particle() {
    let items = vec![json!({
        "type": "book",
        "id": "p1",
        "title": "T",
        "author": [{
            "family": "Silva",
            "given": "Maria",
            "non-dropping-particle": "da"
        }],
        "issued": {"date-parts": [[2024]]}
    })];

    let bib = csl_json_array_to_bibtex(&items);
    assert!(
        bib.contains("da Silva"),
        "bibtex must keep the particle:\n{bib}"
    );

    let biblatex = csl_json_array_to_biblatex(&items);
    assert!(
        biblatex.contains("da Silva"),
        "biblatex must keep the particle:\n{biblatex}"
    );

    let ris = csl_json_to_ris(&items[0]);
    assert!(
        ris.contains("da Silva"),
        "ris must keep the particle:\n{ris}"
    );

    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml).expect("hayagriva YAML must load");
    let author = &lib.get("p1").unwrap().authors().unwrap()[0];
    assert_eq!(
        (author.name.as_str(), author.prefix.as_deref()),
        ("Silva", Some("da")),
        "hayagriva must keep the particle:\n{yaml}"
    );
}

// ── Hayagriva keys ───────────────────────────────────────────────────
//
// Items without an `id` all took the literal key `entry`, and `id: ""`
// emitted an empty key. Duplicate YAML keys mean the last item silently wins.

#[test]
fn hayagriva_keys_are_unique_and_non_empty() {
    let items = vec![
        json!({"type": "book", "title": "A"}),
        json!({"type": "book", "title": "B"}),
        json!({"id": "", "type": "book", "title": "C"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml);
    assert!(lib.is_ok(), "must load: {:?}\n{yaml}", lib.err());
    assert_eq!(
        lib.unwrap().len(),
        items.len(),
        "every item needs its own key:\n{yaml}"
    );
}

// ── Every field is escaped ───────────────────────────────────────────
//
// 0.3.7/0.3.8 escaped titles and names but interpolated volume, number,
// pages, identifiers and eprint fields raw ("numeric/simple values"). A `}`
// in any of them closed the field early: the entry re-imported with zero
// entries and one error — the whole reference lost.

fn hostile_field_item() -> Value {
    json!({
        "type": "article-journal",
        "id": "f1",
        "title": "Real Title",
        "volume": "1},\n  title = {pwned",
        "issue": "2}",
        "page": "3{",
        "DOI": "10.1000/x}y",
        "URL": "https://example.org/a{b}\\c",
        "ISBN": "978}",
        "ISSN": "1234-567{",
        "PMID": "1}",
        "PMCID": "PMC1}",
        "chapter-number": "4}",
        "custom": {"eprint": {"id": "2301.1}", "type": "arxiv}", "class": "cs}"}},
        "issued": {"date-parts": [[2024]]}
    })
}

#[test]
fn bibtex_and_biblatex_escape_every_field() {
    let items = vec![hostile_field_item()];
    for (name, bib) in [
        ("bibtex", csl_json_array_to_bibtex(&items)),
        ("biblatex", csl_json_array_to_biblatex(&items)),
    ] {
        let res = parse_bibtex(&bib, &ParseOptions::default());
        assert_eq!(
            (res.entries.len(), res.errors.len()),
            (1, 0),
            "{name} must re-import as one clean entry:\n{bib}"
        );
        assert_eq!(res.entries[0]["title"], "Real Title", "{name}:\n{bib}");
        assert!(
            !bib.contains("title = {pwned"),
            "{name}: volume must not open a field:\n{bib}"
        );
        assert!(
            bib.contains("doi = {10.1000/x%7Dy}"),
            "{name}: verbatim braces are percent-encoded:\n{bib}"
        );
    }
}

// ── String-or-number CSL variables ───────────────────────────────────
//
// CSL-JSON allows numbers for volume/issue/page/edition and numeric strings
// in date-parts (`[["2019", "5"]]`, which Zotero and citation-js emit).
// Reading them with as_str()/as_i64() dropped them from every exporter.

fn numeric_item() -> Value {
    json!({
        "type": "article-journal",
        "title": "T",
        "author": [{"family": "Smith", "given": "J"}],
        "volume": 42,
        "issue": 3,
        "page": 7,
        "edition": 2,
        "PMID": 12345678,
        "issued": {"date-parts": [["2019", "5", "3"]]}
    })
}

#[test]
fn exports_keep_numeric_fields_and_string_date_parts() {
    let item = numeric_item();

    let bib = csl_json_to_bibtex(&item);
    for want in [
        "@article{Smith2019,",
        "volume = {42}",
        "number = {3}",
        "pages = {7}",
        "edition = {2}",
        "pmid = {12345678}",
        "year = {2019}",
        "month = may",
    ] {
        assert!(bib.contains(want), "bibtex missing `{want}`:\n{bib}");
    }

    let blx = csl_json_array_to_biblatex(std::slice::from_ref(&item));
    for want in ["volume = {42}", "date = {2019-05-03}"] {
        assert!(blx.contains(want), "biblatex missing `{want}`:\n{blx}");
    }

    let ris = csl_json_to_ris(&item);
    for want in [
        "VL  - 42",
        "IS  - 3",
        "SP  - 7",
        "PY  - 2019",
        "DA  - 2019/05/03/",
    ] {
        assert!(ris.contains(want), "ris missing `{want}`:\n{ris}");
    }

    let yaml = csl_json_array_to_hayagriva(std::slice::from_ref(&item));
    for want in ["volume: \"42\"", "date: 2019-05-03"] {
        assert!(yaml.contains(want), "hayagriva missing `{want}`:\n{yaml}");
    }
    hayagriva::io::from_yaml_str(&yaml).expect("hayagriva YAML must load");
}

#[test]
fn exports_read_raw_iso_dates() {
    let item = json!({"type": "book", "id": "r", "title": "T", "issued": {"raw": "2018-07"}});
    let bib = csl_json_to_bibtex(&item);
    assert!(bib.contains("year = {2018}"), "{bib}");
    assert!(bib.contains("month = jul"), "{bib}");
}

#[test]
fn ambiguous_raw_dates_are_not_read_by_position() {
    // "05/03/2019" is day/month or month/day — never year 5.
    for raw in ["05/03/2019", "March 2019", "5"] {
        let item = json!({"type": "book", "id": "r", "title": "T", "issued": {"raw": raw}});
        let bib = csl_json_to_bibtex(&item);
        assert!(!bib.contains("year ="), "{raw}: {bib}");
    }
    let item =
        json!({"type": "book", "id": "r", "title": "T", "issued": {"raw": "2018-07-15T10:00:00Z"}});
    let blx = csl_json_array_to_biblatex(std::slice::from_ref(&item));
    assert!(blx.contains("date = {2018-07-15}"), "{blx}");
}

#[test]
fn biblatex_thesis_type_round_trips_as_written() {
    // `type = {phdthesis}` came back from our own parser as the genre
    // "phdthesis" ("[Phdthesis]" in APA) and threw away the original text.
    for genre in ["PhD thesis", "Dissertação (Mestrado em Educação)"] {
        let item = json!({"type": "thesis", "id": "t", "title": "T", "genre": genre,
            "publisher": "U", "issued": {"date-parts": [[2020]]}});
        let blx = csl_json_array_to_biblatex(std::slice::from_ref(&item));
        let back = parse_bibtex(&blx, &ParseOptions::default());
        assert_eq!(back.entries[0]["genre"], genre, "{blx}");
    }
}

#[test]
fn out_of_range_date_parts_are_dropped_not_emitted() {
    let item = json!({"type": "book", "id": "d", "title": "T", "issued": {"date-parts": [[2024, 13, 40]]}});
    let ris = csl_json_to_ris(&item);
    assert!(
        !ris.contains("DA  -"),
        "month 13 must not reach RIS:\n{ris}"
    );
    let yaml = csl_json_array_to_hayagriva(std::slice::from_ref(&item));
    assert!(
        yaml.contains("date: 2024\n") || yaml.ends_with("date: 2024"),
        "{yaml}"
    );
    hayagriva::io::from_yaml_str(&yaml).expect("hayagriva YAML must load");
}

// ── Thesis / report institutions ─────────────────────────────────────
//
// BibTeX styles read `school` for theses and `institution` for tech reports
// and ignore `publisher` there; biblatex reads `institution` for both and a
// `type` for theses. Exporting `publisher` dropped the university.

#[test]
fn thesis_and_report_export_their_institution_field() {
    let thesis = json!({"type": "thesis", "id": "t", "title": "T", "publisher": "Univ X",
        "genre": "Dissertação (Mestrado em Educação)", "issued": {"date-parts": [[2020]]}});
    let report = json!({"type": "report", "id": "r", "title": "R", "publisher": "NASA",
        "issued": {"date-parts": [[2020]]}});

    let bib = csl_json_to_bibtex(&thesis);
    assert!(bib.starts_with("@mastersthesis{t,"), "{bib}");
    assert!(bib.contains("school = {Univ X}"), "{bib}");
    assert!(!bib.contains("publisher"), "{bib}");
    let bib = csl_json_to_bibtex(&report);
    assert!(bib.contains("institution = {NASA}"), "{bib}");

    let blx = csl_json_array_to_biblatex(&[thesis.clone(), report]);
    assert!(blx.contains("institution = {Univ X}"), "{blx}");
    assert!(
        blx.contains("type = {Dissertação (Mestrado em Educação)}"),
        "{blx}"
    );
    assert!(blx.contains("institution = {NASA}"), "{blx}");
    assert!(!blx.contains("publisher"), "{blx}");

    // Still the publisher after a round trip through our own parser.
    let back = parse_bibtex(&csl_json_to_bibtex(&thesis), &ParseOptions::default());
    assert_eq!(back.entries[0]["publisher"], "Univ X");
}

// ── Cite keys never collide ──────────────────────────────────────────
//
// The `a`/`b` suffix ignored keys already in the file: an item whose own id
// was `smith2024a` and a repeated `smith2024` both came out `smith2024a`, a
// repeated key BibTeX rejects. CiteMe passes keys from imported `.bib`
// files through as ids, so `a`/`b` keys are the common case, not an edge.
// Classic BibTeX also matches keys without regard to case.

fn cite_keys(bib: &str) -> Vec<String> {
    bib.lines()
        .filter_map(|line| line.strip_prefix('@'))
        .filter_map(|line| {
            Some(
                line[line.find('{')? + 1..]
                    .trim_end_matches(',')
                    .to_string(),
            )
        })
        .collect()
}

#[test]
fn cite_keys_never_collide_with_a_key_already_in_the_file() {
    let book = |id: &str, title: &str| json!({"type": "book", "id": id, "title": title});
    let cases = [
        (
            vec![
                book("smith2024", "A"),
                book("smith2024a", "B"),
                book("smith2024", "C"),
            ],
            ["smith2024", "smith2024a", "smith2024b"],
        ),
        (
            vec![
                book("smith2024", "A"),
                book("smith2024", "B"),
                book("smith2024a", "C"),
            ],
            ["smith2024", "smith2024b", "smith2024a"],
        ),
        (
            vec![
                book("Smith2024", "A"),
                book("smith2024", "B"),
                book("SMITH2024", "C"),
            ],
            ["Smith2024", "smith2024a", "SMITH2024b"],
        ),
    ];
    for (items, want) in cases {
        for (name, out) in [
            ("bibtex", csl_json_array_to_bibtex(&items)),
            ("biblatex", csl_json_array_to_biblatex(&items)),
        ] {
            assert_eq!(cite_keys(&out), want, "{name}:\n{out}");
            let back = parse_bibtex(&out, &ParseOptions::default());
            assert_eq!(
                (back.entries.len(), back.errors.len()),
                (3, 0),
                "{name}:\n{out}"
            );
        }
    }
}

// ── BibTeX name parts ────────────────────────────────────────────────
//
// A name part holding a comma or the word "and" went out bare, and BibTeX
// reads both as structure: "Procter and Gamble" re-imported as two authors,
// the family name "Smith, Jones" as family "Smith" with suffix "Jones".

#[test]
fn name_parts_with_a_comma_or_and_stay_one_name() {
    let item = json!({"type": "book", "id": "n", "title": "T", "author": [
        {"family": "Procter and Gamble", "given": "X"},
        {"family": "Smith, Jones", "given": "Ann"},
        {"family": "Doe", "given": "Mary AND John"},
        {"family": "Silva", "given": "Maria", "non-dropping-particle": "da"},
        {"family": "Anderson", "given": "Andrea"}
    ]});
    for (name, bib) in [
        ("bibtex", csl_json_to_bibtex(&item)),
        (
            "biblatex",
            csl_json_array_to_biblatex(std::slice::from_ref(&item)),
        ),
    ] {
        let back = parse_bibtex(&bib, &ParseOptions::default());
        assert_eq!(back.errors.len(), 0, "{name}:\n{bib}");
        let authors = &back.entries[0]["author"];
        assert_eq!(
            authors.as_array().map(Vec::len),
            Some(5),
            "{name}: one name became several:\n{bib}\n=> {authors}"
        );
        assert_eq!(authors[0]["family"], "Procter and Gamble", "{name}:\n{bib}");
        assert_eq!(authors[1]["family"], "Smith, Jones", "{name}:\n{bib}");
        assert_eq!(authors[1]["given"], "Ann", "{name}:\n{bib}");
        assert_eq!(authors[2]["given"], "Mary AND John", "{name}:\n{bib}");
        assert_eq!(authors[3]["family"], "Silva", "{name}:\n{bib}");
        assert_eq!(authors[4]["family"], "Anderson", "{name}:\n{bib}");
    }
    // "and" inside a word is not the separator; nothing to protect there.
    let bib = csl_json_to_bibtex(&item);
    assert!(bib.contains(" and Anderson, Andrea}"), "{bib}");
}

// ── Numeric ids ──────────────────────────────────────────────────────
//
// CSL-JSON allows numeric ids. The exporters read `id` as a string only,
// so `"id": 7` lost its identity: BibTeX keyed it by author and year, the
// Hayagriva YAML as `entry`.

#[test]
fn numeric_ids_keep_their_identity_as_keys() {
    let item = json!({"type": "book", "id": 7, "title": "T",
        "author": [{"family": "Smith"}], "issued": {"date-parts": [[2024]]}});
    let items = std::slice::from_ref(&item);
    let bib = csl_json_to_bibtex(&item);
    assert!(bib.starts_with("@book{7,"), "{bib}");
    let blx = csl_json_array_to_biblatex(items);
    assert!(blx.starts_with("@book{7,"), "{blx}");
    let yaml = csl_json_array_to_hayagriva(items);
    let lib = hayagriva::io::from_yaml_str(&yaml).expect("hayagriva YAML must load");
    assert!(lib.get("7").is_some(), "{yaml}");
}

// ── CSL `number` ─────────────────────────────────────────────────────
//
// A report or patent number (CSL `number`) reached no exporter. In BibTeX
// and BibLaTeX it is a report's `number`; an article's `number` is its
// article number, which both keep in `eid` because `number` is the issue
// there. RIS has `M1` for it, and `C7` for an article number; Hayagriva
// reads it from `serial-number.serial`.

#[test]
fn csl_number_reaches_every_exporter() {
    let report = json!({"type": "report", "id": "r", "title": "Report", "number": "TR-42",
        "publisher": "MIT", "issued": {"date-parts": [[2020]]}});
    let article = json!({"type": "article-journal", "id": "a", "title": "A",
        "container-title": "PLOS ONE", "issue": "3", "number": "e0123456",
        "issued": {"date-parts": [[2020]]}});
    let items = [report.clone(), article.clone()];

    for (name, bib) in [
        ("bibtex", csl_json_array_to_bibtex(&items)),
        ("biblatex", csl_json_array_to_biblatex(&items)),
    ] {
        assert!(bib.contains("number = {TR-42}"), "{name}:\n{bib}");
        assert!(bib.contains("number = {3}"), "{name}:\n{bib}");
        assert!(bib.contains("eid = {e0123456}"), "{name}:\n{bib}");
        let back = parse_bibtex(&bib, &ParseOptions::default());
        assert_eq!(
            (back.entries.len(), back.errors.len()),
            (2, 0),
            "{name}:\n{bib}"
        );
        assert!(
            back.entries[0].to_string().contains("TR-42"),
            "{name}: the report number must survive a round trip:\n{bib}"
        );
    }

    let ris = csl_json_to_ris(&report);
    assert!(ris.contains("M1  - TR-42"), "{ris}");
    let ris = csl_json_to_ris(&article);
    assert!(ris.contains("C7  - e0123456"), "{ris}");
    assert!(ris.contains("IS  - 3"), "{ris}");

    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml).expect("hayagriva YAML must load");
    assert_eq!(
        lib.get("r").and_then(|e| e.keyed_serial_number("serial")),
        Some("TR-42"),
        "{yaml}"
    );
}

// ── Fields CiteMe emits ──────────────────────────────────────────────
//
// CiteMe stamps `accessed` on every webpage and sends `event-title` for
// conference papers; imported `@software` entries carry a `version`. Our
// importers read BibLaTeX `urldate`/`eventtitle`/`version` and RIS
// `Y2`/`ED`/`A2`, but no exporter wrote them, so every round trip through a
// file dropped them.

fn webpage_accessed() -> Value {
    json!({"type": "webpage", "id": "w", "title": "Page", "URL": "https://x.org",
        "issued": {"date-parts": [[2020]]}, "accessed": {"date-parts": [[2024, 5, 3]]}})
}

#[test]
fn biblatex_round_trips_accessed_event_title_and_version() {
    let items = vec![
        webpage_accessed(),
        json!({"type": "paper-conference", "id": "c", "title": "Paper",
            "container-title": "Proceedings of X", "event-title": "X 2022",
            "issued": {"date-parts": [[2022]]}}),
        json!({"type": "software", "id": "s", "title": "Tool", "version": "1.2.0",
            "issued": {"date-parts": [[2022]]}}),
    ];
    let bib = csl_json_array_to_biblatex(&items);
    let back = parse_bibtex(&bib, &ParseOptions::default());
    assert_eq!((back.entries.len(), back.errors.len()), (3, 0), "{bib}");
    assert_eq!(
        back.entries[0]["accessed"],
        json!({"date-parts": [[2024, 5, 3]]}),
        "{bib}"
    );
    assert_eq!(back.entries[1]["event-title"], "X 2022", "{bib}");
    assert_eq!(back.entries[2]["version"], "1.2.0", "{bib}");
}

#[test]
fn bibtex_round_trips_accessed_as_urldate() {
    // Not a classic BibTeX field, but the one Zotero's BibTeX export writes
    // and JabRef reads; styles that don't know it ignore it.
    let bib = csl_json_to_bibtex(&webpage_accessed());
    assert!(bib.contains("urldate = {2024-05-03}"), "{bib}");
    let back = parse_bibtex(&bib, &ParseOptions::default());
    assert_eq!(
        back.entries[0]["accessed"],
        json!({"date-parts": [[2024, 5, 3]]}),
        "{bib}"
    );
}

#[test]
fn ris_round_trips_accessed_and_editors() {
    let ris = csl_json_to_ris(&webpage_accessed());
    let back = parse_ris(&ris, &ParseOptions::default());
    assert_eq!(
        back.entries[0]["accessed"],
        json!({"date-parts": [[2024, 5, 3]]}),
        "{ris}"
    );

    // A chapter's editors edit the host book: `A2`. Anything else: `ED`.
    for ty in ["chapter", "book"] {
        let item = json!({"type": ty, "id": "e", "title": "T",
            "author": [{"family": "Author", "given": "A"}],
            "editor": [{"family": "Editor", "given": "E"}, {"literal": "Board of X"}],
            "container-title": "The Book", "issued": {"date-parts": [[2020]]}});
        let ris = csl_json_to_ris(&item);
        let back = parse_ris(&ris, &ParseOptions::default());
        assert_eq!(
            back.entries[0]["editor"],
            json!([{"family": "Editor", "given": "E"}, {"literal": "Board of X"}]),
            "{ty}:\n{ris}"
        );
    }
}

// ── RIS types, identifiers and keywords ──────────────────────────────

#[test]
fn ris_exports_patents_as_pat() {
    let item = json!({"type": "patent", "id": "p", "title": "Widget", "number": "US123"});
    let ris = csl_json_to_ris(&item);
    assert!(ris.starts_with("TY  - PAT\n"), "{ris}");
}

#[test]
fn ris_keeps_numeric_identifiers() {
    // `ISBN: 9780306406157` is valid CSL-JSON; RIS read it with `as_str`.
    let item = json!({"type": "book", "id": "k", "title": "T",
        "ISBN": 9780306406157u64, "ISSN": 12345678});
    let ris = csl_json_to_ris(&item);
    assert!(ris.contains("SN  - 9780306406157"), "{ris}");
    assert!(ris.contains("SN  - 12345678"), "{ris}");
}

#[test]
fn ris_keyword_list_items_stay_whole() {
    // MeSH headings carry commas. A list item is one keyword; only the
    // comma-separated string form is split.
    let item = json!({"type": "article-journal", "id": "k", "title": "T",
        "keyword": ["Carcinoma, Non-Small-Cell Lung", "Humans"]});
    let ris = csl_json_to_ris(&item);
    assert_eq!(ris.matches("KW  - ").count(), 2, "{ris}");
    assert!(
        ris.contains("KW  - Carcinoma, Non-Small-Cell Lung\n"),
        "{ris}"
    );

    let item = json!({"type": "article-journal", "id": "k", "title": "T", "keyword": "ml, nlp"});
    assert_eq!(csl_json_to_ris(&item).matches("KW  - ").count(), 2);
}

// ── Years outside 1000–9999 ──────────────────────────────────────────
//
// biblatex writes a BC year as `-0350`: `date = {-350}` re-imported as no
// date at all, with zero errors. A year past 9999 has no form either format
// reads back (`year = {20240}` makes our own parser reject the entry), so it
// is left out rather than written unreadable.

#[test]
fn bc_and_five_digit_years_never_export_an_unreadable_date() {
    for (parts, want) in [
        (json!([[-350]]), Some(json!([[-350]]))),
        (json!([[-350, 3]]), Some(json!([[-350, 3]]))),
        (json!([[50]]), Some(json!([[50]]))),
        (json!([[20240]]), None),
    ] {
        let item =
            json!({"type": "book", "id": "k", "title": "T", "issued": {"date-parts": parts}});
        for (name, bib) in [
            ("bibtex", csl_json_to_bibtex(&item)),
            (
                "biblatex",
                csl_json_array_to_biblatex(std::slice::from_ref(&item)),
            ),
        ] {
            let back = parse_bibtex(&bib, &ParseOptions::default());
            assert_eq!(
                (back.entries.len(), back.errors.len()),
                (1, 0),
                "{name} {parts}:\n{bib}"
            );
            assert_eq!(
                back.entries[0]
                    .get("issued")
                    .map(|d| d["date-parts"].clone()),
                want,
                "{name} {parts}:\n{bib}"
            );
        }
    }
}

// ── Hayagriva YAML must always load ──────────────────────────────────
//
// One item hayagriva can't read fails the whole file, so every value that
// reaches a typed field has to be one it accepts.

/// Load `item` next to a plain one; both must come back.
fn load_hayagriva(item: Value) -> (hayagriva::Library, String) {
    let items = vec![json!({"id": "ok", "type": "book", "title": "Fine"}), item];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml)
        .unwrap_or_else(|e| panic!("hayagriva YAML must load: {e}\n{yaml}"));
    assert_eq!(lib.len(), 2, "{yaml}");
    (lib, yaml)
}

#[test]
fn hayagriva_persons_load_back_as_the_same_names() {
    // Persons went out as "Given Family"; hayagriva reads a bare string as
    // "Family, Given", so "John Smith" came back with no given name, "Maria
    // da Silva" as family "Silva" with prefix "Maria da", and a literal with
    // three commas made the whole file fail to load.
    let (lib, yaml) = load_hayagriva(json!({"id": "k", "type": "book", "title": "T",
        "author": [
            {"family": "Smith", "given": "John"},
            {"family": "King", "given": "Martin Luther", "suffix": "Jr."},
            {"family": "Silva", "given": "Maria", "non-dropping-particle": "da"},
            {"literal": "Ministério da Saúde, Secretaria de Atenção à Saúde"},
            {"literal": "Johnson, Smith, Brown, and Co."},
            {"given": "Plato"}
        ],
        "editor": [{"family": "Doe", "given": "Jane"}]}));
    let entry = lib.get("k").unwrap();
    let names = |persons: &[hayagriva::types::Person]| -> Vec<_> {
        persons
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.given_name.clone(),
                    p.prefix.clone(),
                    p.suffix.clone(),
                )
            })
            .collect()
    };
    let s = |v: &str| Some(v.to_string());
    assert_eq!(
        names(entry.authors().expect("authors")),
        vec![
            ("Smith".into(), s("John"), None, None),
            ("King".into(), s("Martin Luther"), None, s("Jr.")),
            ("Silva".into(), s("Maria"), s("da"), None),
            (
                "Ministério da Saúde, Secretaria de Atenção à Saúde".into(),
                None,
                None,
                None
            ),
            ("Johnson, Smith, Brown, and Co.".into(), None, None, None),
            ("Plato".into(), None, None, None),
        ],
        "{yaml}"
    );
    assert_eq!(
        names(entry.editors().expect("editors")),
        vec![("Doe".into(), s("Jane"), None, None)],
        "{yaml}"
    );
}

#[test]
fn hayagriva_dates_hayagriva_cannot_read_never_break_the_file() {
    // hayagriva reads a four-digit year (`800-03` failed the file), a day
    // that exists in its month (`2019-02-30` failed it), and only a bare
    // year past 9999.
    for (parts, want) in [
        (json!([[800, 3]]), (800, Some(3), None)),
        (json!([[50]]), (50, None, None)),
        (json!([[-350]]), (-350, None, None)),
        (json!([[2019, 2, 30]]), (2019, Some(2), None)),
        (json!([[2020, 2, 29]]), (2020, Some(2), Some(29))),
        (json!([[20240, 3]]), (20240, None, None)),
    ] {
        let (lib, yaml) = load_hayagriva(
            json!({"id": "k", "type": "book", "title": "T", "issued": {"date-parts": parts}}),
        );
        let date = *lib.get("k").unwrap().date().expect("date kept");
        assert_eq!(
            (
                date.year,
                date.month.map(|m| m + 1),
                date.day.map(|d| d + 1)
            ),
            want,
            "{parts}:\n{yaml}"
        );
    }
}

#[test]
fn hayagriva_text_with_characters_yaml_rejects_round_trips() {
    // libyaml refuses U+FFFE/U+FFFF anywhere in the stream — escaped, they
    // load. One such character in one title failed the whole file.
    for text in [
        "a\u{FFFE}b",
        "a\u{FFFF}b",
        "a\u{7F}b",
        "a\u{85}b",
        "a\u{0}b",
    ] {
        let (lib, yaml) = load_hayagriva(json!({"id": "k", "type": "book", "title": text,
            "author": [{"family": text, "given": "J"}], "publisher": text}));
        let entry = lib.get("k").unwrap();
        assert_eq!(
            entry.title().map(|t| t.to_string()).as_deref(),
            Some(text),
            "{text:?}:\n{yaml}"
        );
        assert_eq!(entry.authors().unwrap()[0].name, text, "{text:?}:\n{yaml}");
    }
}

#[test]
fn hayagriva_urls_hayagriva_cannot_parse_are_left_out() {
    // hayagriva parses `url` as an absolute URL: "www.example.org/page"
    // failed the whole file. Guessing a scheme would invent data.
    for url in [
        "www.example.org/page",
        "doi.org/10.1000/xyz",
        "/papers/1.pdf",
        "https://exa mple.org/a",
    ] {
        let (lib, yaml) =
            load_hayagriva(json!({"id": "k", "type": "webpage", "title": "T", "URL": url}));
        assert!(lib.get("k").unwrap().url().is_none(), "{url}:\n{yaml}");
    }
    for url in [
        "https://example.org/a b?c=1&d=2",
        "mailto:someone@example.org",
    ] {
        let (lib, yaml) =
            load_hayagriva(json!({"id": "k", "type": "webpage", "title": "T", "URL": url}));
        assert!(lib.get("k").unwrap().url().is_some(), "{url}:\n{yaml}");
    }
}

#[test]
fn hayagriva_keys_never_collide_with_an_explicit_id() {
    let items = vec![
        json!({"id": "x", "type": "book", "title": "A"}),
        json!({"id": "x", "type": "book", "title": "B"}),
        json!({"id": "x-2", "type": "book", "title": "C"}),
        json!({"type": "book", "title": "D"}),
        json!({"type": "book", "title": "E"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml).expect("hayagriva YAML must load");
    let keys: Vec<String> = lib.iter().map(|e| e.key().to_string()).collect();
    assert_eq!(keys, ["x", "x-3", "x-2", "entry", "entry-2"], "{yaml}");
}

#[test]
fn hayagriva_key_dedup_is_linear() {
    // Each id-less item probed `entry`, `entry-2`, … from the start again:
    // 16,000 of them took 15 s in a release build, 8,000 about 11 s in a
    // debug one. Linear, this is tens of milliseconds.
    let items: Vec<Value> = (0..8_000)
        .map(|_| json!({"type": "book", "title": "T"}))
        .collect();
    let start = std::time::Instant::now();
    let yaml = csl_json_array_to_hayagriva(&items);
    let elapsed = start.elapsed();
    assert!(yaml.contains("entry-8000:"), "every item keyed");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "8,000 keys took {elapsed:?}"
    );
}
