use super::{ParseOptions, ParseResult, ParseErrorInfo};

/// Validate and pass through CSL-JSON input.
///
/// Accepts either a single CSL-JSON object or an array of objects.
/// Each item is validated by deserializing into `citationberg::json::Item`
/// — if it succeeds, the item is valid CSL-JSON. Invalid items are reported
/// in the errors array with a preview and error message.
///
/// This is a "parser" in the sense that it normalizes input (single → array)
/// and validates structure, but does not convert between formats.
pub fn parse_csl_json(input: &str, options: &ParseOptions) -> ParseResult {
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
                preview: format!("Input size {} bytes exceeds limit {} bytes",
                    input.len(), options.max_input_bytes),
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

        // Validate by attempting to deserialize as citationberg::json::Item
        match serde_json::from_value::<hayagriva::citationberg::json::Item>(item.clone()) {
            Ok(_) => entries.push(item),
            Err(e) => {
                let preview = serde_json::to_string(&item)
                    .unwrap_or_else(|_| format!("{item:?}"));
                errors.push(ParseErrorInfo {
                    preview: preview.chars().take(80).collect(),
                    error: format!("invalid CSL-JSON item: {e}"),
                });
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
        // The string "not an object" fails citationberg validation
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_max_entries() {
        let input = r#"[
            {"type": "article-journal", "title": "A"},
            {"type": "article-journal", "title": "B"},
            {"type": "article-journal", "title": "C"}
        ]"#;
        let opts = ParseOptions { max_entries: Some(2), ..Default::default() };
        let result = parse_csl_json(input, &opts);
        assert_eq!(result.entries.len(), 2);
        assert!(result.truncated);
    }
}
