use serde_json::{json, Value};
use super::{ParseOptions, ParseResult, ParseErrorInfo};

/// RIS type tag → CSL-JSON type mapping
fn ris_type_to_csl(ty: &str) -> &'static str {
    match ty {
        "JOUR" | "JFULL" => "article-journal",
        "BOOK" | "WHOLE" => "book",
        "CHAP" | "CHAPT" => "chapter",
        "THES" => "thesis",
        "CONF" | "CPAPER" => "paper-conference",
        "RPRT" | "REPORT" => "report",
        "ELEC" | "ICOMM" => "webpage",
        "DATA" | "DBASE" => "dataset",
        _ => "article-journal",
    }
}

/// Parse RIS content into CSL-JSON items.
pub fn parse_ris(input: &str, options: &ParseOptions) -> ParseResult {
    if input.trim().is_empty() {
        return ParseResult {
            entries: vec![], errors: vec![],
            format: "ris".to_string(), truncated: false, scanned_entries: 0,
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
            format: "ris".to_string(), truncated: true, scanned_entries: 0,
        };
    }

    let mut entries: Vec<Value> = Vec::new();
    let mut current: Option<serde_json::Map<String, Value>> = None;
    let mut authors: Vec<Value> = Vec::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut scanned = 0;

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }

        // RIS lines: "XX  - value" (tag is first 2-4 chars, then "  - ", then value)
        // Handle "ER  -" (end record) which may lack trailing space
        let (tag, value) = if let Some(pos) = line.find("  - ") {
            (line[..pos].trim(), line[pos + 4..].trim())
        } else if line.starts_with("ER  -") {
            ("ER", "")
        } else {
            continue
        };

        match tag {
            "TY" => {
                current = Some(serde_json::Map::new());
                authors.clear();
                keywords.clear();
                if let Some(ref mut entry) = current {
                    entry.insert("type".into(), json!(ris_type_to_csl(value)));
                }
            }
            "ER" => {
                if let Some(mut entry) = current.take() {
                    if !authors.is_empty() {
                        entry.insert("author".into(), json!(authors.clone()));
                    }
                    if !keywords.is_empty() {
                        entry.insert("keyword".into(), json!(keywords.join(", ")));
                    }
                    scanned += 1;
                    if let Some(max) = options.max_entries {
                        if entries.len() >= max {
                            return ParseResult {
                                entries, errors: vec![],
                                format: "ris".to_string(), truncated: true, scanned_entries: scanned,
                            };
                        }
                    }
                    entries.push(Value::Object(entry));
                }
                authors.clear();
                keywords.clear();
            }
            _ => {
                if let Some(ref mut entry) = current {
                    match tag {
                        "AU" | "A1" => {
                            let parts: Vec<&str> = value.splitn(2, ',').collect();
                            if parts.len() == 2 {
                                authors.push(json!({"family": parts[0].trim(), "given": parts[1].trim()}));
                            } else {
                                authors.push(json!({"literal": value}));
                            }
                        }
                        "TI" | "T1" => { entry.insert("title".into(), json!(value)); }
                        "JO" | "JF" | "T2" => { entry.insert("container-title".into(), json!(value)); }
                        "PY" | "Y1" => {
                            let year_str = value.split('/').next().unwrap_or(value);
                            if let Ok(year) = year_str.parse::<i32>() {
                                entry.insert("issued".into(), json!({"date-parts": [[year]]}));
                            }
                        }
                        "VL" => { entry.insert("volume".into(), json!(value)); }
                        "IS" => { entry.insert("issue".into(), json!(value)); }
                        "SP" => {
                            let ep = entry.get("_ep").and_then(|v| v.as_str()).unwrap_or("");
                            if !ep.is_empty() {
                                entry.insert("page".into(), json!(format!("{value}-{ep}")));
                            } else {
                                entry.insert("_sp".into(), json!(value));
                            }
                        }
                        "EP" => {
                            let sp = entry.get("_sp").and_then(|v| v.as_str()).unwrap_or("");
                            if !sp.is_empty() {
                                entry.insert("page".into(), json!(format!("{sp}-{value}")));
                                entry.remove("_sp");
                            } else {
                                entry.insert("_ep".into(), json!(value));
                            }
                        }
                        "DO" => { entry.insert("DOI".into(), json!(value)); }
                        "UR" => { entry.insert("URL".into(), json!(value)); }
                        "PB" => { entry.insert("publisher".into(), json!(value)); }
                        "CY" => { entry.insert("publisher-place".into(), json!(value)); }
                        "SN" => {
                            if value.contains('-') && value.len() == 9 {
                                entry.insert("ISSN".into(), json!(value));
                            } else {
                                entry.insert("ISBN".into(), json!(value));
                            }
                        }
                        "AB" | "N2" => { entry.insert("abstract".into(), json!(value)); }
                        "LA" => { entry.insert("language".into(), json!(value)); }
                        "KW" => { keywords.push(value.to_string()); }
                        _ => {}
                    }
                }
            }
        }
    }

    // Handle entry without trailing ER
    if let Some(mut entry) = current.take() {
        if !authors.is_empty() {
            entry.insert("author".into(), json!(authors));
        }
        scanned += 1;
        entries.push(Value::Object(entry));
    }

    // Clean up internal fields (_sp, _ep)
    for entry in &mut entries {
        if let Some(obj) = entry.as_object_mut() {
            obj.remove("_sp");
            obj.remove("_ep");
        }
    }

    ParseResult {
        entries, errors: vec![],
        format: "ris".to_string(), truncated: false, scanned_entries: scanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ris_basic() {
        let input = include_str!("../../../../tests/fixtures/samples/sample.ris");
        let result = parse_ris(input, &ParseOptions::default());

        assert_eq!(result.entries.len(), 2, "should parse 2 entries");
        assert!(result.errors.is_empty());

        let first = &result.entries[0];
        assert_eq!(first["type"], "article-journal");
        assert_eq!(first["title"], "A Study of Something");
    }

    #[test]
    fn test_parse_ris_empty() {
        let result = parse_ris("", &ParseOptions::default());
        assert!(result.entries.is_empty());
    }
}
