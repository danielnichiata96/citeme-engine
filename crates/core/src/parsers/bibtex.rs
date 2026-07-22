use std::borrow::Cow;

use super::{ParseErrorInfo, ParseOptions, ParseResult};
use biblatex::{Bibliography as BibBib, ChunksExt};
use hayagriva::io::from_biblatex_str;
use hayagriva::Entry;
use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;

fn combining_latex_command(mark: char) -> Option<&'static str> {
    match mark {
        '\u{300}' => Some("`"),
        '\u{301}' => Some("'"),
        '\u{302}' => Some("^"),
        '\u{303}' => Some("~"),
        '\u{304}' => Some("="),
        '\u{306}' => Some("u"),
        '\u{307}' => Some("."),
        '\u{308}' => Some("\""),
        '\u{30A}' => Some("r"),
        '\u{30B}' => Some("H"),
        '\u{30C}' => Some("v"),
        '\u{323}' => Some("d"),
        '\u{327}' => Some("c"),
        '\u{328}' => Some("k"),
        '\u{332}' => Some("b"),
        '\u{338}' => Some("o"),
        _ => None,
    }
}

fn normalize_bibtex_input(input: &str) -> Cow<'_, str> {
    let mut normalized = None;
    let mut chars = input.char_indices().peekable();
    let mut last = 0;

    while let Some((idx, ch)) = chars.next() {
        if ch != '\\' {
            continue;
        }

        let Some(&(next_idx, next_ch)) = chars.peek() else {
            continue;
        };
        let Some(command) = combining_latex_command(next_ch) else {
            continue;
        };

        let out = normalized.get_or_insert_with(|| String::with_capacity(input.len()));
        out.push_str(&input[last..idx]);
        out.push('\\');
        out.push_str(command);
        last = next_idx + next_ch.len_utf8();
        chars.next();
    }

    match normalized {
        Some(mut out) => {
            out.push_str(&input[last..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(input),
    }
}

fn normalize_display_text(text: &str) -> String {
    text.nfc().collect()
}

fn bib_field(entry: &biblatex::Entry, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        entry
            .get(key)
            .map(|chunks| normalize_display_text(chunks.format_verbatim().trim()))
            .filter(|value| !value.is_empty())
    })
}

fn insert_string_if_missing(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<String>,
) {
    if obj
        .get(key)
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
    {
        return;
    }
    if let Some(value) = value.filter(|s| !s.trim().is_empty()) {
        obj.insert(key.into(), json!(value));
    }
}

fn date_parts_from_isoish(input: &str) -> Option<Value> {
    let mut parts = Vec::new();
    for raw in input.split(['-', '/']).take(3) {
        if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        let Ok(part) = raw.parse::<i32>() else { break };
        parts.push(part);
    }
    if parts.is_empty() {
        return None;
    }
    Some(json!({ "date-parts": [parts] }))
}

fn biblatex_type_to_csl(entry: &biblatex::Entry) -> Option<&'static str> {
    use biblatex::EntryType as T;

    match &entry.entry_type {
        T::Article => {
            if bib_field(entry, &["journaltitle", "journal"]).is_some() {
                Some("article-journal")
            } else {
                Some("article")
            }
        }
        T::Book | T::MvBook => Some("book"),
        T::InBook | T::InCollection | T::SuppBook | T::SuppCollection | T::InReference => {
            Some("chapter")
        }
        T::InProceedings | T::Proceedings | T::MvProceedings => Some("paper-conference"),
        T::MastersThesis | T::PhdThesis | T::Thesis => Some("thesis"),
        T::TechReport | T::Report => Some("report"),
        T::Online => Some("webpage"),
        T::Software => Some("software"),
        T::Dataset => Some("dataset"),
        T::Patent => Some("patent"),
        _ => None,
    }
}

/// Convert a Hayagriva Entry to a CSL-JSON serde_json::Value.
///
/// This is the Entry → CSL-JSON direction (used by parsers).
/// The formatting hot path uses CSL-JSON → BibliographyDriver directly.
fn entry_to_csl_json(entry: &Entry) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // Type mapping: Hayagriva EntryType → CSL-JSON type string
    let csl_type = match entry.entry_type() {
        hayagriva::types::EntryType::Article => {
            if entry
                .parents()
                .iter()
                .any(|p| *p.entry_type() == hayagriva::types::EntryType::Periodical)
            {
                "article-journal"
            } else if entry
                .parents()
                .iter()
                .any(|p| *p.entry_type() == hayagriva::types::EntryType::Proceedings)
            {
                "paper-conference"
            } else if entry
                .parents()
                .iter()
                .any(|p| *p.entry_type() == hayagriva::types::EntryType::Newspaper)
            {
                "article-newspaper"
            } else {
                "article"
            }
        }
        hayagriva::types::EntryType::Book => "book",
        hayagriva::types::EntryType::Chapter | hayagriva::types::EntryType::Anthos => "chapter",
        hayagriva::types::EntryType::Thesis => "thesis",
        hayagriva::types::EntryType::Report => "report",
        hayagriva::types::EntryType::Web | hayagriva::types::EntryType::Blog => "webpage",
        hayagriva::types::EntryType::Repository => "software",
        _ => "article",
    };
    obj.insert("type".into(), json!(csl_type));
    obj.insert("id".into(), json!(entry.key()));

    // Title
    if let Some(title) = entry.title() {
        let title = title.to_string();
        obj.insert("title".into(), json!(normalize_display_text(&title)));
    }

    // Authors
    if let Some(authors) = entry.authors() {
        let names: Vec<serde_json::Value> = authors
            .iter()
            .map(|p| {
                if let Some(given) = &p.given_name {
                    let mut name_obj = json!({
                        "family": normalize_display_text(&p.name),
                        "given": normalize_display_text(given),
                    });
                    if let Some(prefix) = &p.prefix {
                        name_obj["dropping-particle"] = json!(normalize_display_text(prefix));
                    }
                    if let Some(suffix) = &p.suffix {
                        name_obj["suffix"] = json!(normalize_display_text(suffix));
                    }
                    name_obj
                } else {
                    json!({"literal": normalize_display_text(&p.name)})
                }
            })
            .collect();
        if !names.is_empty() {
            obj.insert("author".into(), json!(names));
        }
    }

    // Editors
    if let Some(editors) = entry.editors() {
        let names: Vec<serde_json::Value> = editors
            .iter()
            .map(|p| {
                if let Some(given) = &p.given_name {
                    let mut name_obj = json!({
                        "family": normalize_display_text(&p.name),
                        "given": normalize_display_text(given),
                    });
                    if let Some(prefix) = &p.prefix {
                        name_obj["dropping-particle"] = json!(normalize_display_text(prefix));
                    }
                    if let Some(suffix) = &p.suffix {
                        name_obj["suffix"] = json!(normalize_display_text(suffix));
                    }
                    name_obj
                } else {
                    json!({"literal": normalize_display_text(&p.name)})
                }
            })
            .collect();
        if !names.is_empty() {
            obj.insert("editor".into(), json!(names));
        }
    }

    // Date
    if let Some(date) = entry.date() {
        let mut parts: Vec<i32> = vec![date.year];
        if let Some(m) = date.month {
            parts.push(m as i32);
            if let Some(d) = date.day {
                parts.push(d as i32);
            }
        }
        obj.insert("issued".into(), json!({"date-parts": [parts]}));
    }

    // Container title (from first parent)
    if let Some(parent) = entry.parents().first() {
        if let Some(title) = parent.title() {
            let title = title.to_string();
            obj.insert(
                "container-title".into(),
                json!(normalize_display_text(&title)),
            );
        }
    }

    // Identifiers
    if let Some(doi) = entry.doi() {
        obj.insert("DOI".into(), json!(doi));
    }
    if let Some(isbn) = entry.isbn() {
        obj.insert("ISBN".into(), json!(isbn));
    }
    if let Some(issn) = entry.issn() {
        obj.insert("ISSN".into(), json!(issn));
    }
    if let Some(url) = entry.url_any() {
        obj.insert("URL".into(), json!(url.value.to_string()));
    }

    // Biblio fields
    if let Some(v) = entry.volume() {
        obj.insert("volume".into(), json!(v.to_string()));
    }
    if let Some(v) = entry.issue() {
        obj.insert("issue".into(), json!(v.to_string()));
    }
    if let Some(v) = entry.page_range() {
        obj.insert("page".into(), json!(v.to_string()));
    }
    if let Some(v) = entry.edition() {
        obj.insert("edition".into(), json!(v.to_string()));
    }

    // Abstract / note (FormatString::to_string strips markup)
    if let Some(s) = entry.abstract_() {
        let value = s.to_string();
        obj.insert("abstract".into(), json!(normalize_display_text(&value)));
    }
    if let Some(n) = entry.note() {
        let value = n.to_string();
        obj.insert("note".into(), json!(normalize_display_text(&value)));
    }

    // Chapter number
    if let Some(c) = entry.chapter() {
        obj.insert("chapter-number".into(), json!(c.to_string()));
    }

    // PMID / PMCID — CSL 1.0.1+ top-level variables
    if let Some(v) = entry.pmid() {
        obj.insert("PMID".into(), json!(v));
    }
    if let Some(v) = entry.pmcid() {
        obj.insert("PMCID".into(), json!(v));
    }

    // arXiv → custom.eprint (CSL has no top-level arxiv variable)
    if let Some(v) = entry.arxiv() {
        let custom = obj.entry("custom").or_insert_with(|| json!({}));
        if let Some(map) = custom.as_object_mut() {
            map.insert("eprint".into(), json!({ "id": v, "type": "arxiv" }));
        }
    }

    // Publisher
    if let Some(pub_) = entry.publisher() {
        if let Some(name) = pub_.name() {
            let value = name.to_string();
            obj.insert("publisher".into(), json!(normalize_display_text(&value)));
        }
        if let Some(loc) = pub_.location() {
            let value = loc.to_string();
            obj.insert(
                "publisher-place".into(),
                json!(normalize_display_text(&value)),
            );
        }
    }

    // Genre
    if let Some(genre) = entry.genre() {
        let value = genre.to_string();
        obj.insert("genre".into(), json!(normalize_display_text(&value)));
    }

    // Language
    if let Some(lang) = entry.language() {
        let value = lang.to_string();
        obj.insert("language".into(), json!(normalize_display_text(&value)));
    }

    serde_json::Value::Object(obj)
}

/// Collect biblatex entries from `input`, tolerating duplicate cite keys and
/// individual malformed entries.
///
/// `biblatex::Bibliography::parse` bails with `DuplicateKey` on collisions,
/// and with whatever first grammar error it hits on broken input. Hayagriva
/// (the primary parser) is more tolerant in both cases, so a single bad entry
/// or one accidental duplicate would otherwise wipe out the merge for every
/// usable entry in the file. Falling back to per-chunk parse salvages the rest.
fn collect_biblatex_entries(input: &str) -> Vec<biblatex::Entry> {
    if let Ok(bib) = BibBib::parse(input) {
        return bib.into_iter().collect();
    }

    let mut out = Vec::new();
    for chunk in input.split("\n@") {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let entry_str = if chunk.starts_with('@') {
            chunk.to_string()
        } else {
            format!("@{chunk}")
        };
        let lower = entry_str.to_lowercase();
        if lower.starts_with("@preamble")
            || lower.starts_with("@string")
            || lower.starts_with("@comment")
        {
            continue;
        }
        if let Ok(bib) = BibBib::parse(&entry_str) {
            out.extend(bib.into_iter());
        }
    }
    out
}

/// Merge fields that Hayagriva's BibLaTeX interop drops — specifically
/// `keywords`, `series`, standalone `pmid`/`pmcid`, and
/// `eprint`/`eprinttype`/`eprintclass` — by re-parsing `input` with the
/// `biblatex` crate and matching entries by cite key.
///
/// Opção B from the plan: pragmatic dual-parse until upstream Hayagriva
/// exposes these on `Entry`. O(n) lookup-map build + O(1) merge per entry.
fn merge_biblatex_extras(entries: &mut [Value], input: &str) {
    let bib_entries = collect_biblatex_entries(input);
    if bib_entries.is_empty() {
        return;
    }

    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, e) in bib_entries.iter().enumerate() {
        index.entry(e.key.as_str()).or_insert(i);
    }

    for entry_csl in entries.iter_mut() {
        let Some(key) = entry_csl.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(&idx) = index.get(key) else { continue };
        let bib_entry = &bib_entries[idx];
        let Some(obj) = entry_csl.as_object_mut() else {
            continue;
        };

        if let Some(csl_type) = biblatex_type_to_csl(bib_entry) {
            obj.insert("type".into(), json!(csl_type));
        }

        insert_string_if_missing(
            obj,
            "container-title",
            bib_field(
                bib_entry,
                &["journaltitle", "journal", "booktitle", "eventtitle"],
            ),
        );
        insert_string_if_missing(obj, "event-title", bib_field(bib_entry, &["eventtitle"]));
        insert_string_if_missing(
            obj,
            "publisher-place",
            bib_field(bib_entry, &["location", "address"]),
        );
        insert_string_if_missing(
            obj,
            "publisher",
            bib_field(bib_entry, &["publisher", "institution", "school"]),
        );
        insert_string_if_missing(obj, "volume", bib_field(bib_entry, &["volume"]));
        insert_string_if_missing(obj, "issue", bib_field(bib_entry, &["number", "issue"]));
        insert_string_if_missing(obj, "page", bib_field(bib_entry, &["pages"]));
        insert_string_if_missing(obj, "URL", bib_field(bib_entry, &["url"]));
        insert_string_if_missing(obj, "DOI", bib_field(bib_entry, &["doi"]));
        insert_string_if_missing(obj, "version", bib_field(bib_entry, &["version"]));

        if !obj.contains_key("accessed") {
            if let Some(date) = bib_field(bib_entry, &["urldate"]) {
                if let Some(date_parts) = date_parts_from_isoish(&date) {
                    obj.insert("accessed".into(), date_parts);
                }
            }
        }

        // keywords: CSL uses `keyword` (singular, comma-separated string per v1.0 schema)
        if !obj.contains_key("keyword") {
            if let Ok(chunks) = bib_entry.keywords() {
                let raw = chunks.format_verbatim();
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    obj.insert("keyword".into(), json!(normalize_display_text(trimmed)));
                }
            }
        }

        // series → collection-title. Hayagriva nests series under a synthetic
        // parent whose type inherits from the main parent (e.g. Anthology for
        // incollection), making it awkward to traverse. Reading the raw field
        // is simpler and matches what BibTeX users expect.
        if !obj.contains_key("collection-title") {
            if let Ok(chunks) = bib_entry.series() {
                let raw = chunks.format_verbatim();
                let v = normalize_display_text(raw.trim());
                if !v.is_empty() {
                    obj.insert("collection-title".into(), json!(v));
                }
            }
        }

        // Direct `pmid`/`pmcid` fields: hayagriva 0.9 only reads these from
        // `eprint`/`eprinttype = pubmed`; a standalone `pmid = {...}` is dropped.
        // Fill from biblatex Entry::get() when not already present.
        if !obj.contains_key("PMID") {
            if let Some(chunks) = bib_entry.get("pmid") {
                let v = chunks.format_verbatim().trim().to_string();
                if !v.is_empty() {
                    obj.insert("PMID".into(), json!(v));
                }
            }
        }
        if !obj.contains_key("PMCID") {
            if let Some(chunks) = bib_entry.get("pmcid") {
                let v = chunks.format_verbatim().trim().to_string();
                if !v.is_empty() {
                    obj.insert("PMCID".into(), json!(v));
                }
            }
        }

        // eprint / eprinttype / eprintclass → custom.eprint = { id, type, class }
        //
        // Require both id AND type to emit. An `eprint = {…}` without
        // `eprinttype` is meaningless to downstream styles (they need the type
        // to route to arxiv, pubmed, etc.); emitting a type-less `custom.eprint`
        // would leak into our BibTeX exporter as a broken `eprint = {…}`
        // without `eprinttype = {…}` that many style files choke on.
        //
        // If hayagriva already set `custom.eprint` (via `entry.arxiv()` when
        // `eprinttype = arxiv`), augment with `class` but preserve id/type.
        if let Ok(id) = bib_entry.eprint() {
            let id = id.trim().to_string();
            let eprint_type = bib_entry
                .eprint_type()
                .ok()
                .map(|c| c.format_verbatim().trim().to_lowercase())
                .filter(|s| !s.is_empty());
            let eprint_class = bib_entry
                .eprint_class()
                .ok()
                .map(|c| c.format_verbatim().trim().to_string())
                .filter(|s| !s.is_empty());

            // Require id + type to avoid leaking incomplete `custom.eprint`
            // into downstream exports. If hayagriva already populated
            // `custom.eprint` via `entry.arxiv()`, this branch also fires
            // (biblatex reads the same `eprinttype` field), and we layer
            // `class` in via the map-merge below without clobbering id/type.
            if !id.is_empty() && eprint_type.is_some() {
                let custom = obj.entry("custom").or_insert_with(|| json!({}));
                if let Some(map) = custom.as_object_mut() {
                    let mut eprint_obj = match map.get("eprint").cloned() {
                        Some(Value::Object(m)) => m,
                        _ => serde_json::Map::new(),
                    };
                    eprint_obj.entry("id").or_insert_with(|| json!(id));
                    if let Some(t) = eprint_type {
                        eprint_obj.entry("type").or_insert_with(|| json!(t));
                    }
                    if let Some(c) = eprint_class {
                        eprint_obj.insert("class".into(), json!(c));
                    }
                    if !eprint_obj.is_empty() {
                        map.insert("eprint".into(), Value::Object(eprint_obj));
                    }
                }
            }
        }
    }
}

/// Parse BibTeX/BibLaTeX content into CSL-JSON.
pub fn parse_bibtex(input: &str, options: &ParseOptions) -> ParseResult {
    if input.trim().is_empty() {
        return ParseResult {
            entries: vec![],
            errors: vec![],
            format: "bibtex".to_string(),
            truncated: false,
            scanned_entries: 0,
        };
    }

    // DoS guard: reject oversized input before parsing
    if input.len() > options.max_input_bytes {
        return ParseResult {
            entries: vec![],
            errors: vec![ParseErrorInfo {
                preview: format!(
                    "Input size {} bytes exceeds limit {} bytes",
                    input.len(),
                    options.max_input_bytes
                ),
                error: "input too large".to_string(),
            }],
            format: "bibtex".to_string(),
            truncated: true,
            scanned_entries: 0,
        };
    }

    let normalized_input = normalize_bibtex_input(input);
    let input = normalized_input.as_ref();

    match from_biblatex_str(input) {
        Ok(library) => {
            let mut entries = Vec::new();
            let total = library.len();
            let mut truncated = false;

            for entry in library.iter() {
                if let Some(max) = options.max_entries {
                    if entries.len() >= max {
                        truncated = true;
                        break;
                    }
                }

                entries.push(entry_to_csl_json(entry));
            }

            // Merge fields hayagriva drops (keywords, eprint/type/class).
            merge_biblatex_extras(&mut entries, input);

            ParseResult {
                entries,
                errors: vec![],
                format: "bibtex".to_string(),
                truncated,
                scanned_entries: total,
            }
        }
        Err(_) => {
            // Whole-file parse failed. Attempt per-entry recovery.
            let mut entries = Vec::new();
            let mut errors = Vec::new();
            let mut scanned = 0;
            let mut truncated = false;

            for chunk in input.split("\n@") {
                let chunk = chunk.trim();
                if chunk.is_empty() {
                    continue;
                }

                let entry_str = if chunk.starts_with('@') {
                    chunk.to_string()
                } else {
                    format!("@{chunk}")
                };

                let lower = entry_str.to_lowercase();
                if lower.starts_with("@preamble")
                    || lower.starts_with("@string")
                    || lower.starts_with("@comment")
                {
                    continue;
                }

                scanned += 1;
                if let Some(max) = options.max_entries {
                    if entries.len() >= max {
                        truncated = true;
                        break;
                    }
                }

                match from_biblatex_str(&entry_str) {
                    Ok(lib) => {
                        for entry in lib.iter() {
                            entries.push(entry_to_csl_json(entry));
                        }
                    }
                    Err(e) => {
                        if errors.len() < crate::parsers::MAX_PARSE_ERRORS {
                            errors.push(ParseErrorInfo {
                                preview: entry_str.chars().take(80).collect(),
                                error: e
                                    .first()
                                    .map(|e| format!("{e}"))
                                    .unwrap_or_default()
                                    .to_string(),
                            });
                        }
                    }
                }
            }

            // Recovery path: `merge_biblatex_extras` falls through to
            // per-chunk biblatex parsing when the whole-file parse fails, so
            // individual recovered entries still get their keywords/series/
            // pmid/eprint fields filled in.
            merge_biblatex_extras(&mut entries, input);

            ParseResult {
                entries,
                errors,
                format: "bibtex".to_string(),
                truncated,
                scanned_entries: scanned,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bibtex_basic() {
        let input = include_str!("../../../../tests/fixtures/samples/sample.bib");
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(result.entries.len(), 2, "should parse 2 entries");
        assert!(
            result.errors.is_empty(),
            "should have no errors: {:?}",
            result.errors
        );
        assert!(!result.truncated);

        let first = &result.entries[0];
        let title = first.get("title").and_then(|v| v.as_str());
        assert!(title.is_some(), "first entry should have a title");
    }

    #[test]
    fn test_parse_bibtex_empty() {
        let result = parse_bibtex("", &ParseOptions::default());
        assert!(result.entries.is_empty());
    }

    #[test]
    fn test_parse_bibtex_malformed() {
        let input = "@article{bad, title = {unclosed brace";
        let result = parse_bibtex(input, &ParseOptions::default());
        assert!(
            !result.entries.is_empty() || !result.errors.is_empty(),
            "malformed input should produce either partial entries or errors, not empty result"
        );
    }

    #[test]
    fn test_parse_bibtex_preserves_metadata() {
        let input = include_str!("../../../../tests/fixtures/samples/metadata-rich.bib");
        let result = parse_bibtex(input, &ParseOptions::default());
        assert_eq!(
            result.entries.len(),
            3,
            "should parse 3 entries: {:?}",
            result.errors
        );
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );

        let first = &result.entries[0];
        assert_eq!(first["abstract"], "This paper studies Z.");
        assert_eq!(first["note"], "Funded by NSF");
        // CSL `keyword` is singular, string (per CSL v1.0 schema). Only the
        // biblatex-merge path populates this (hayagriva doesn't expose keywords).
        let kw = first["keyword"].as_str().expect("keyword must be a string");
        assert!(
            kw.contains("machine learning") && kw.contains("NLP"),
            "keyword should carry both tags: {kw}"
        );
        // PMID/PMCID top-level. hayagriva only reads PMID from `eprinttype=pubmed`;
        // this fixture has `eprinttype=arxiv`, so PMID/PMCID are load-bearing on
        // the merge path specifically.
        assert_eq!(first["PMID"], "38123456");
        assert_eq!(first["PMCID"], "PMC9876543");
        // custom.eprint populated either by hayagriva.arxiv() (id+type) or the
        // merge (id+type+class). `class` is the merge-only signal.
        assert_eq!(first["custom"]["eprint"]["id"], "2301.12345");
        assert_eq!(first["custom"]["eprint"]["type"], "arxiv");
        assert_eq!(
            first["custom"]["eprint"]["class"], "cs.CL",
            "eprintclass is only reachable via the biblatex merge path: {}",
            first["custom"]
        );

        let second = &result.entries[1];
        assert_eq!(second["chapter-number"], "7");
        assert_eq!(
            second["collection-title"], "Studies in Knowledge",
            "series should map to collection-title: {second}"
        );

        // Legacy `journal = {…}` (classic BibTeX) still parses as article with
        // container-title and keywords picked up by the merge.
        let third = &result.entries[2];
        assert_eq!(third["type"], "article-journal");
        assert_eq!(third["container-title"], "Old Journal");
        assert_eq!(third["keyword"], "legacy");
    }

    #[test]
    fn test_parse_biblatex_article_fields() {
        let input = r#"@article{smith2023,
  author = {Smith, Jane},
  title = {Deep Learning},
  journaltitle = {Nature},
  date = {2023-05-17},
  volume = {617},
  number = {7960},
  pages = {100--115},
  doi = {10.1038/s41586-023-12345},
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "expected one entry: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["type"], "article-journal");
        assert_eq!(first["title"], "Deep Learning");
        assert_eq!(first["container-title"], "Nature");
        assert_eq!(first["issued"]["date-parts"][0][0], 2023);
        assert_eq!(first["volume"], "617");
        assert_eq!(first["issue"], "7960");
        assert_eq!(first["page"], "100-115");
        assert_eq!(first["DOI"], "10.1038/s41586-023-12345");
    }

    #[test]
    fn test_parse_biblatex_conference_event_and_location() {
        let input = r#"@inproceedings{foo2022,
  author = {Foo, A.},
  title = {Talk Title},
  eventtitle = {NeurIPS 2022},
  booktitle = {Proceedings of NeurIPS},
  date = {2022-12},
  location = {New Orleans},
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "expected one entry: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["type"], "paper-conference");
        assert_eq!(first["container-title"], "Proceedings of NeurIPS");
        assert_eq!(first["event-title"], "NeurIPS 2022");
        assert_eq!(first["publisher-place"], "New Orleans");
    }

    #[test]
    fn test_parse_biblatex_dataset_and_software_types() {
        let input = r#"@dataset{data2023,
  author = {Lab, Some},
  title = {Dataset X},
  publisher = {Zenodo},
  date = {2023},
  doi = {10.5281/zenodo.1234567},
}

@software{tool2024,
  author = {Dev Team},
  title = {A Tool},
  version = {1.2.0},
  date = {2024-03-01},
  url = {https://github.com/x/y},
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            2,
            "expected two entries: {:?}",
            result.errors
        );
        let dataset = &result.entries[0];
        assert_eq!(dataset["type"], "dataset");
        assert_eq!(dataset["DOI"], "10.5281/zenodo.1234567");
        assert_eq!(dataset["publisher"], "Zenodo");

        let software = &result.entries[1];
        assert_eq!(software["type"], "software");
        assert_eq!(software["title"], "A Tool");
        assert_eq!(software["URL"], "https://github.com/x/y");
        assert_eq!(software["version"], "1.2.0");
    }

    #[test]
    fn test_parse_bibtex_duplicate_keys_do_not_break_merge() {
        // biblatex::Bibliography::parse returns DuplicateKey and bails, while
        // hayagriva tolerates duplicates. Without the chunk-level fallback in
        // collect_biblatex_entries, the presence of any duplicate would silently
        // wipe out keywords/series/pmid/eprintclass for every other entry.
        let input = r#"@article{smith2024,
  author = {Smith},
  title = {A},
  journal = {X},
  year = {2024},
  keywords = {ml}
}

@article{smith2024,
  author = {Smith},
  title = {B},
  journal = {Y},
  year = {2024}
}

@article{doe2023,
  author = {Doe},
  title = {C},
  journal = {Z},
  year = {2023},
  keywords = {nlp}
}
"#;
        let result = parse_bibtex(input, &ParseOptions::default());
        // hayagriva keeps both smith entries + doe = 3.
        assert_eq!(
            result.entries.len(),
            3,
            "hayagriva tolerates dupes: {:?}",
            result.errors
        );
        // The non-duplicated `doe2023` must still have its keyword merged even
        // though `smith2024` is duplicated earlier in the file.
        let doe = result
            .entries
            .iter()
            .find(|e| e["id"] == "doe2023")
            .expect("doe2023 present");
        assert_eq!(
            doe["keyword"], "nlp",
            "merge must salvage keywords on entries with unique keys even when \
             the file contains duplicate-key entries elsewhere"
        );
    }

    #[test]
    fn test_parse_bibtex_eprint_without_type_is_dropped() {
        // An `eprint = {…}` with no `eprinttype` is ambiguous (arxiv? pubmed?
        // hdl?) — emitting custom.eprint with only `id` would leak into exports
        // as broken fields. The parser must drop it.
        let input = r#"@article{x,
  author = {X},
  title = {T},
  journal = {J},
  year = {2024},
  eprint = {something.12345}
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        let first = &result.entries[0];
        assert!(
            first["custom"]["eprint"].is_null(),
            "eprint without eprinttype must not populate custom.eprint: {}",
            first["custom"]
        );
    }

    #[test]
    fn test_parse_bibtex_normalizes_combining_latex_commands() {
        let input = "@article{x,\n  title = {Aspectos jur\\\u{301}idicos e Jo\\\u{303}ao},\n  author = {Silva, Jo\\\u{303}ao and Garc\\\u{301}ia, Mar\\\u{301}ia},\n  journal = {Revista},\n  year = {2024}\n}";
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "expected one parsed entry: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["title"], "Aspectos jurídicos e João");
        assert_eq!(first["author"][0]["family"], "Silva");
        assert_eq!(first["author"][0]["given"], "João");
        assert_eq!(first["author"][1]["family"], "García");
        assert_eq!(first["author"][1]["given"], "María");
    }
}
