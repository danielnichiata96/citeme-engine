use criterion::{criterion_group, criterion_main, Criterion};
use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::types::FormatOptions;

fn setup_engine() -> CitationEngine {
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();
    let style_dir = ws_root.join("tests/fixtures/styles");
    let locale_dir = ws_root.join("tests/fixtures/locales");

    let mut engine = CitationEngine::new();

    // Load all available styles
    for entry in std::fs::read_dir(&style_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "csl") {
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let xml = std::fs::read_to_string(&path).unwrap();
            engine.load_style(&name, &xml).unwrap();
        }
    }

    // Load all available locales
    for entry in std::fs::read_dir(&locale_dir).unwrap() {
        let path = entry.unwrap().path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("locales-") && name.ends_with(".xml") {
                let code = name.strip_prefix("locales-").unwrap().strip_suffix(".xml").unwrap();
                let xml = std::fs::read_to_string(&path).unwrap();
                engine.load_locale(code, &xml).unwrap();
            }
        }
    }

    engine
}

fn bench_format_one(c: &mut Criterion) {
    let engine = setup_engine();
    let opts = FormatOptions::default();

    let csl_json = r#"{
        "type": "article-journal",
        "id": "bench1",
        "title": "A Study of Something Important in Modern Science",
        "author": [
            {"family": "Smith", "given": "John A."},
            {"family": "Doe", "given": "Jane B."}
        ],
        "issued": {"date-parts": [[2024]]},
        "container-title": "Journal of Important Studies",
        "volume": "42",
        "issue": "3",
        "page": "100-115",
        "DOI": "10.1234/jis.2024.001"
    }"#;

    c.bench_function("format_one_apa", |b| {
        b.iter(|| engine.format_one(csl_json, "apa", "en-US", &opts).unwrap())
    });
}

fn bench_format_one_abnt(c: &mut Criterion) {
    let engine = setup_engine();
    let opts = FormatOptions {
        abnt_post_process: true,
        ..Default::default()
    };

    let csl_json = r#"{
        "type": "article-journal",
        "id": "bench_abnt",
        "title": "Impactos ambientais na Amazônia brasileira",
        "author": [{"family": "Silva", "given": "João Pedro"}],
        "issued": {"date-parts": [[2024]]},
        "container-title": "Revista Brasileira de Ecologia",
        "volume": "28",
        "issue": "2",
        "page": "45-60"
    }"#;

    c.bench_function("format_one_abnt", |b| {
        b.iter(|| engine.format_one(csl_json, "abnt", "pt-BR", &opts).unwrap())
    });
}

fn bench_format_batch_50(c: &mut Criterion) {
    let engine = setup_engine();
    let opts = FormatOptions::default();

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();
    let batch_json = std::fs::read_to_string(
        ws_root.join("tests/fixtures/samples/batch-50.json")
    ).unwrap();

    c.bench_function("format_batch_50_apa", |b| {
        b.iter(|| engine.format_batch(&batch_json, "apa", "en-US", &opts).unwrap())
    });
}

fn bench_format_batch_sizes(c: &mut Criterion) {
    let engine = setup_engine();
    let opts = FormatOptions::default();

    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();
    let all_items: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(ws_root.join("tests/fixtures/samples/batch-50.json")).unwrap()
    ).unwrap();

    let mut group = c.benchmark_group("format_batch_sizes");
    for size in [1, 5, 10, 25, 50] {
        let subset: Vec<&serde_json::Value> = all_items.iter().take(size).collect();
        let json = serde_json::to_string(&subset).unwrap();

        group.bench_function(format!("batch_{size}"), |b| {
            b.iter(|| engine.format_batch(&json, "apa", "en-US", &opts).unwrap())
        });
    }
    group.finish();
}

fn bench_parse_bibtex(c: &mut Criterion) {
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    let small_bib = std::fs::read_to_string(
        ws_root.join("tests/fixtures/samples/sample.bib")
    ).unwrap();
    let large_bib = std::fs::read_to_string(
        ws_root.join("tests/fixtures/samples/large-1000.bib")
    ).unwrap();

    let default_opts = citeme_engine_core::parsers::ParseOptions::default();

    let mut group = c.benchmark_group("parse_bibtex");

    group.bench_function("2_entries", |b| {
        b.iter(|| citeme_engine_core::parsers::bibtex::parse_bibtex(&small_bib, &default_opts))
    });

    group.bench_function("1000_entries", |b| {
        b.iter(|| citeme_engine_core::parsers::bibtex::parse_bibtex(&large_bib, &default_opts))
    });

    group.finish();
}

fn bench_parse_ris(c: &mut Criterion) {
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    let ris = std::fs::read_to_string(
        ws_root.join("tests/fixtures/samples/sample.ris")
    ).unwrap();

    let default_opts = citeme_engine_core::parsers::ParseOptions::default();

    c.bench_function("parse_ris_2_entries", |b| {
        b.iter(|| citeme_engine_core::parsers::ris::parse_ris(&ris, &default_opts))
    });
}

fn bench_style_loading(c: &mut Criterion) {
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();
    let apa_xml = std::fs::read_to_string(
        ws_root.join("tests/fixtures/styles/apa.csl")
    ).unwrap();

    c.bench_function("load_style_apa", |b| {
        b.iter(|| {
            let mut engine = CitationEngine::new();
            engine.load_style("apa", &apa_xml).unwrap();
        })
    });
}

criterion_group!(
    benches,
    bench_format_one,
    bench_format_one_abnt,
    bench_format_batch_50,
    bench_format_batch_sizes,
    bench_parse_bibtex,
    bench_parse_ris,
    bench_style_loading,
);
criterion_main!(benches);
