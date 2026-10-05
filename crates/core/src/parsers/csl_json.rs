use super::{ParseErrorInfo, ParseOptions, ParseResult};
use crate::csl_item::{self, ItemProblem};

/// Validate and pass through CSL-JSON input.
///
/// Accepts either a single CSL-JSON object or an array of objects. Each
/// item is validated the way the formatter reads it (`csl_item::prepare`):
/// malformed items — a name that is a string, a date part that is not a
/// number — are reported in the errors array with a preview and error
/// message. Valid items pass through — including what the renderer can't
/// use, such as extension objects like `custom` and date ranges — with the
/// value types CSL-JSON defines (`csl_item::well_typed`).
///
/// This is a "parser" in the sense that it normalizes input (single → array)
/// and validates structure, but does not convert between formats.
pub fn parse_csl_json(input: &str, options: &ParseOptions) -> ParseResult {
    // A byte-order mark is not JSON; Windows tools write one.
    let input = input.trim_start_matches('\u{FEFF}');
    if input.trim().is_empty() {
        return ParseResult {
            entries: vec![],
            errors: vec![],
            format: "csl-json".to_string(),
            truncated: false,
            scanned_entries: 0,
        };
    }

    // DoS guard
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
            format: "csl-json".to_string(),
            truncated: true,
            scanned_entries: 0,
        };
    }

    // Try parsing as array first, then as single object
    let items: Vec<serde_json::Value> = match serde_json::from_str(input) {
        Ok(serde_json::Value::Array(arr)) => arr,
        Ok(obj @ serde_json::Value::Object(_)) => vec![obj],
        Ok(_) => {
            return ParseResult {
                entries: vec![],
                errors: vec![ParseErrorInfo {
                    preview: input.chars().take(80).collect(),
                    error: "expected a JSON object or array".to_string(),
                }],
                format: "csl-json".to_string(),
                truncated: false,
                scanned_entries: 0,
            };
        }
        Err(e) => {
            return ParseResult {
                entries: vec![],
                errors: vec![ParseErrorInfo {
                    preview: input.chars().take(80).collect(),
                    error: format!("invalid JSON: {e}"),
                }],
                format: "csl-json".to_string(),
                truncated: false,
                scanned_entries: 0,
            };
        }
    };

    let total = items.len();
    let mut entries = Vec::new();
    let mut errors = Vec::new();

    for item in items {
        if let Some(max) = options.max_entries {
            if entries.len() >= max {
                return ParseResult {
                    entries,
                    errors,
                    format: "csl-json".to_string(),
                    truncated: true,
                    scanned_entries: total,
                };
            }
        }

        match csl_item::prepare(&item) {
            Ok(_) | Err(ItemProblem::Unsupported(_)) => entries.push(csl_item::well_typed(&item)),
            Err(ItemProblem::Invalid(e)) => {
                if errors.len() < crate::parsers::MAX_PARSE_ERRORS {
                    let preview =
                        serde_json::to_string(&item).unwrap_or_else(|_| format!("{item:?}"));
                    errors.push(ParseErrorInfo {
                        preview: preview.chars().take(80).collect(),
                        error: format!("invalid CSL-JSON item: {e}"),
                    });
                }
            }
        }
    }

    ParseResult {
        entries,
        errors,
        format: "csl-json".to_string(),
        truncated: false,
        scanned_entries: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_item() {
        let input = r#"{"type": "article-journal", "title": "Test", "id": "test1"}"#;
        let result = parse_csl_json(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.format, "csl-json");
    }

    #[test]
    fn test_parse_array() {
        let input = r#"[
            {"type": "article-journal", "title": "Test 1"},
            {"type": "book", "title": "Test 2"}
        ]"#;
        let result = parse_csl_json(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 2);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_csl_json("", &ParseOptions::default());
        assert!(result.entries.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_csl_json("{not valid json", &ParseOptions::default());
        assert!(result.entries.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].error.contains("invalid JSON"));
    }

    #[test]
    fn test_parse_mixed_valid_invalid() {
        let input = r#"[
            {"type": "article-journal", "title": "Good"},
            "not an object"
        ]"#;
        let result = parse_csl_json(input, &ParseOptions::default());
        // The string "not an object" is not a CSL-JSON item
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn valid_items_the_renderer_cannot_hold_are_accepted() {
        // citationberg's `Item` rejected each of these whole, though they are
        // valid CSL-JSON — `custom` is how CiteMe (and this engine's BibTeX
        // import) carry an arXiv id, so their own exports didn't re-import.
        // Entries keep the value types that check used to guarantee: no
        // `null`s or booleans, keyword lists as the CSL string form, no null
        // names, numbers as integers or text. CiteMe's converter calls
        // `keyword.split`, and reads name parts as strings.
        let items = serde_json::json!([
            {"type": "article-journal", "title": "A", "keyword": ["x", "y"]},
            {"type": "article-journal", "title": "B", "DOI": null, "flag": true},
            {"type": "article-journal", "title": "C",
             "custom": {"eprint": {"id": "1706.03762", "type": "arxiv"}}},
            {"type": "book", "title": "D", "issued": {"literal": "forthcoming"}},
            {"type": "book", "title": "E", "issued": {"date-parts": [[2019], [2020]]}},
            {"type": "book", "title": "F", "volume": 1.5,
             "author": [null, {"family": "Doe", "given": null}]}
        ]);
        let result = parse_csl_json(&items.to_string(), &ParseOptions::default());
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            serde_json::Value::Array(result.entries),
            serde_json::json!([
                {"type": "article-journal", "title": "A", "keyword": "x, y"},
                {"type": "article-journal", "title": "B"},
                {"type": "article-journal", "title": "C",
                 "custom": {"eprint": {"id": "1706.03762", "type": "arxiv"}}},
                {"type": "book", "title": "D", "issued": {"literal": "forthcoming"}},
                {"type": "book", "title": "E", "issued": {"date-parts": [[2019], [2020]]}},
                {"type": "book", "title": "F", "volume": "1.5", "author": [{"family": "Doe"}]}
            ])
        );
    }

    #[test]
    fn malformed_items_are_rejected_naming_the_variable() {
        let items = serde_json::json!([
            {"type": "book", "title": "A", "author": "Doe, Jane"},
            {"type": "book", "title": "B", "issued": {"date-parts": [["n.d."]]}}
        ]);
        let result = parse_csl_json(&items.to_string(), &ParseOptions::default());
        assert!(result.entries.is_empty());
        assert_eq!(result.errors.len(), 2);
        assert!(
            result.errors[0].error.contains("author"),
            "{:?}",
            result.errors
        );
        assert!(
            result.errors[1].error.contains("issued"),
            "{:?}",
            result.errors
        );
    }

    #[test]
    fn a_byte_order_mark_is_not_content() {
        // Windows tools prefix one; detection sees past it, so the parser
        // must too — it failed with "expected value at line 1 column 1".
        let result = parse_csl_json(
            "\u{FEFF}[{\"type\": \"book\", \"title\": \"T\"}]",
            &ParseOptions::default(),
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.entries.len(), 1);
    }

    #[test]
    fn test_max_entries() {
        let input = r#"[
            {"type": "article-journal", "title": "A"},
            {"type": "article-journal", "title": "B"},
            {"type": "article-journal", "title": "C"}
        ]"#;
        let opts = ParseOptions {
            max_entries: Some(2),
            ..Default::default()
        };
        let result = parse_csl_json(input, &opts);
        assert_eq!(result.entries.len(), 2);
        assert!(result.truncated);
    }
}
