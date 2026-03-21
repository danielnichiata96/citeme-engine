use serde_json::json;
use super::{ParseOptions, ParseResult, ParseErrorInfo};
use hayagriva::io::from_biblatex_str;
use hayagriva::Entry;

/// Convert a Hayagriva Entry to a CSL-JSON serde_json::Value.
///
/// This is the Entry → CSL-JSON direction (used by parsers).
/// The formatting hot path uses CSL-JSON → BibliographyDriver directly.
fn entry_to_csl_json(entry: &Entry) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // Type mapping: Hayagriva EntryType → CSL-JSON type string
    let csl_type = match entry.entry_type() {
        hayagriva::types::EntryType::Article => {
            if entry.parents().iter().any(|p| *p.entry_type() == hayagriva::types::EntryType::Periodical) {
                "article-journal"
            } else if entry.parents().iter().any(|p| *p.entry_type() == hayagriva::types::EntryType::Proceedings) {
                "paper-conference"
            } else if entry.parents().iter().any(|p| *p.entry_type() == hayagriva::types::EntryType::Newspaper) {
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
        obj.insert("title".into(), json!(title.to_string()));
    }

    // Authors
    if let Some(authors) = entry.authors() {
        let names: Vec<serde_json::Value> = authors.iter().map(|p| {
            if let Some(given) = &p.given_name {
                let mut name_obj = json!({"family": p.name, "given": given});
                if let Some(prefix) = &p.prefix {
                    name_obj["dropping-particle"] = json!(prefix);
                }
                if let Some(suffix) = &p.suffix {
                    name_obj["suffix"] = json!(suffix);
                }
                name_obj
            } else {
                json!({"literal": p.name})
            }
        }).collect();
        if !names.is_empty() {
            obj.insert("author".into(), json!(names));
        }
    }

    // Editors
    if let Some(editors) = entry.editors() {
        let names: Vec<serde_json::Value> = editors.iter().map(|p| {
            if let Some(given) = &p.given_name {
                let mut name_obj = json!({"family": p.name, "given": given});
                if let Some(prefix) = &p.prefix {
                    name_obj["dropping-particle"] = json!(prefix);
                }
                if let Some(suffix) = &p.suffix {
                    name_obj["suffix"] = json!(suffix);
                }
                name_obj
            } else {
                json!({"literal": p.name})
            }
        }).collect();
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
            obj.insert("container-title".into(), json!(title.to_string()));
        }
    }

    // Identifiers
    if let Some(doi) = entry.doi() { obj.insert("DOI".into(), json!(doi)); }
    if let Some(isbn) = entry.isbn() { obj.insert("ISBN".into(), json!(isbn)); }
    if let Some(issn) = entry.issn() { obj.insert("ISSN".into(), json!(issn)); }
    if let Some(url) = entry.url_any() { obj.insert("URL".into(), json!(url.value.to_string())); }

    // Biblio fields
    if let Some(v) = entry.volume() { obj.insert("volume".into(), json!(v.to_string())); }
    if let Some(v) = entry.issue() { obj.insert("issue".into(), json!(v.to_string())); }
    if let Some(v) = entry.page_range() { obj.insert("page".into(), json!(v.to_string())); }
    if let Some(v) = entry.edition() { obj.insert("edition".into(), json!(v.to_string())); }

    // Publisher
    if let Some(pub_) = entry.publisher() {
        if let Some(name) = pub_.name() {
            obj.insert("publisher".into(), json!(name.to_string()));
        }
        if let Some(loc) = pub_.location() {
            obj.insert("publisher-place".into(), json!(loc.to_string()));
        }
    }

    // Genre
    if let Some(genre) = entry.genre() {
        obj.insert("genre".into(), json!(genre.to_string()));
    }

    // Language
    if let Some(lang) = entry.language() {
        obj.insert("language".into(), json!(lang.to_string()));
    }

    serde_json::Value::Object(obj)
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
                preview: format!("Input size {} bytes exceeds limit {} bytes",
                    input.len(), options.max_input_bytes),
                error: "input too large".to_string(),
            }],
            format: "bibtex".to_string(),
            truncated: true,
            scanned_entries: 0,
        };
    }

    match from_biblatex_str(input) {
        Ok(library) => {
            let mut entries = Vec::new();
            let total = library.len();

            for entry in library.iter() {
                if let Some(max) = options.max_entries {
                    if entries.len() >= max {
                        return ParseResult {
                            entries,
                            errors: vec![],
                            format: "bibtex".to_string(),
                            truncated: true,
                            scanned_entries: total,
                        };
                    }
                }

                entries.push(entry_to_csl_json(entry));
            }

            ParseResult {
                entries,
                errors: vec![],
                format: "bibtex".to_string(),
                truncated: false,
                scanned_entries: total,
            }
        }
        Err(_) => {
            // Whole-file parse failed. Attempt per-entry recovery.
            let mut entries = Vec::new();
            let mut errors = Vec::new();
            let mut scanned = 0;

            for chunk in input.split("\n@").skip(0) {
                let chunk = chunk.trim();
                if chunk.is_empty() { continue; }

                let entry_str = if chunk.starts_with('@') {
                    chunk.to_string()
                } else {
                    format!("@{chunk}")
                };

                let lower = entry_str.to_lowercase();
                if lower.starts_with("@preamble") || lower.starts_with("@string")
                    || lower.starts_with("@comment") {
                    continue;
                }

                scanned += 1;
                if let Some(max) = options.max_entries {
                    if entries.len() >= max { break; }
                }

                match from_biblatex_str(&entry_str) {
                    Ok(lib) => {
                        for entry in lib.iter() {
                            entries.push(entry_to_csl_json(entry));
                        }
                    }
                    Err(e) => {
                        errors.push(ParseErrorInfo {
                            preview: entry_str.chars().take(80).collect(),
                            error: format!("{}", e.first().map(|e| format!("{e}")).unwrap_or_default()),
                        });
                    }
                }
            }

            ParseResult {
                entries,
                errors,
                format: "bibtex".to_string(),
                truncated: false,
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
        assert!(result.errors.is_empty(), "should have no errors: {:?}", result.errors);
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
}
