use serde_json::Value;

/// CSL-JSON type → RIS TY tag mapping.
fn csl_type_to_ris(csl_type: &str) -> &'static str {
    match csl_type {
        "article-journal" | "article-magazine" => "JOUR",
        "article-newspaper" => "NEWS",
        "book" => "BOOK",
        "chapter" => "CHAP",
        "paper-conference" => "CONF",
        "thesis" => "THES",
        "report" => "RPRT",
        "webpage" | "post-weblog" => "ELEC",
        "dataset" => "DATA",
        "software" => "COMP",
        _ => "JOUR",
    }
}

/// Convert a CSL-JSON item to a RIS record string.
pub fn csl_json_to_ris(item: &Value) -> String {
    let csl_type = item["type"].as_str().unwrap_or("article-journal");
    let mut lines: Vec<String> = Vec::new();

    // TY (type) — must be first
    lines.push(format!("TY  - {}", csl_type_to_ris(csl_type)));

    // Authors
    if let Some(authors) = item["author"].as_array() {
        for author in authors {
            if let Some(literal) = author["literal"].as_str() {
                lines.push(format!("AU  - {literal}"));
            } else {
                let family = author["family"].as_str().unwrap_or("");
                let given = author["given"].as_str().unwrap_or("");
                if given.is_empty() {
                    lines.push(format!("AU  - {family}"));
                } else {
                    lines.push(format!("AU  - {family}, {given}"));
                }
            }
        }
    }

    // Title
    if let Some(v) = item["title"].as_str() { lines.push(format!("TI  - {v}")); }

    // Journal / container
    if let Some(v) = item["container-title"].as_str() {
        let tag = match csl_type {
            "article-journal" | "article-magazine" | "article-newspaper" => "JO",
            _ => "T2",
        };
        lines.push(format!("{tag}  - {v}"));
    }

    // Date — RIS spec: PY for year, DA for full date (YYYY/MM/DD/ format)
    if let Some(dp) = item["issued"]["date-parts"].as_array() {
        if let Some(parts) = dp.first().and_then(|p| p.as_array()) {
            let year = parts.first().and_then(|y| y.as_i64()).unwrap_or(0);
            if year > 0 {
                lines.push(format!("PY  - {year}///"));
                let month = parts.get(1).and_then(|m| m.as_i64());
                let day = parts.get(2).and_then(|d| d.as_i64());
                if month.is_some() {
                    let m = month.unwrap();
                    let d_str = day.map(|d| format!("{d:02}")).unwrap_or_default();
                    lines.push(format!("DA  - {year}/{m:02}/{d_str}/"));
                }
            }
        }
    }

    // Volume, issue
    if let Some(v) = item["volume"].as_str() { lines.push(format!("VL  - {v}")); }
    if let Some(v) = item["issue"].as_str() { lines.push(format!("IS  - {v}")); }

    // Pages: split "100-115" into SP/EP
    if let Some(pages) = item["page"].as_str() {
        let parts: Vec<&str> = pages.splitn(2, |c| c == '-' || c == '–').collect();
        if let Some(sp) = parts.first() {
            lines.push(format!("SP  - {}", sp.trim()));
        }
        if let Some(ep) = parts.get(1) {
            lines.push(format!("EP  - {}", ep.trim()));
        }
    }

    // Identifiers
    if let Some(v) = item["DOI"].as_str() { lines.push(format!("DO  - {v}")); }
    if let Some(v) = item["URL"].as_str() { lines.push(format!("UR  - {v}")); }
    if let Some(v) = item["ISSN"].as_str() { lines.push(format!("SN  - {v}")); }
    if let Some(v) = item["ISBN"].as_str() { lines.push(format!("SN  - {v}")); }

    // Publisher
    if let Some(v) = item["publisher"].as_str() { lines.push(format!("PB  - {v}")); }
    if let Some(v) = item["publisher-place"].as_str() { lines.push(format!("CY  - {v}")); }

    // Abstract
    if let Some(v) = item["abstract"].as_str() { lines.push(format!("AB  - {v}")); }

    // Language
    if let Some(v) = item["language"].as_str() { lines.push(format!("LA  - {v}")); }

    // Keywords — split the normalized comma-separated form (which covers
    // both v1.0.1 string and v1.0.2 array inputs) into one `KW` tag per item.
    if let Some(kw) = super::bibtex::csl_keyword_as_string(item) {
        for k in kw.split(',') {
            let k = k.trim();
            if !k.is_empty() {
                lines.push(format!("KW  - {k}"));
            }
        }
    }

    // ER (end record) — must be last
    lines.push("ER  - ".to_string());

    lines.join("\n")
}

/// Convert multiple CSL-JSON items to a RIS file string.
pub fn csl_json_array_to_ris(items: &[Value]) -> String {
    let mut out = items.iter()
        .map(|item| csl_json_to_ris(item))
        .collect::<Vec<_>>()
        .join("\n\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_export_ris_article() {
        let item = json!({
            "type": "article-journal",
            "title": "A Study",
            "author": [{"family": "Smith", "given": "John"}, {"family": "Doe", "given": "Jane"}],
            "issued": {"date-parts": [[2024]]},
            "container-title": "Journal of Testing",
            "volume": "42",
            "issue": "3",
            "page": "100-115",
            "DOI": "10.1234/test"
        });

        let ris = csl_json_to_ris(&item);
        assert!(ris.starts_with("TY  - JOUR"), "should start with TY: {ris}");
        assert!(ris.contains("AU  - Smith, John"), "should have author: {ris}");
        assert!(ris.contains("AU  - Doe, Jane"), "should have second author: {ris}");
        assert!(ris.contains("TI  - A Study"), "should have title: {ris}");
        assert!(ris.contains("JO  - Journal of Testing"), "should have journal: {ris}");
        assert!(ris.contains("SP  - 100"), "should have start page: {ris}");
        assert!(ris.contains("EP  - 115"), "should have end page: {ris}");
        assert!(ris.ends_with("ER  - "), "should end with ER: {ris}");
    }

    #[test]
    fn test_export_ris_book() {
        let item = json!({
            "type": "book",
            "title": "Great Book",
            "author": [{"family": "Kwan", "given": "Kevin"}],
            "issued": {"date-parts": [[2014]]},
            "publisher": "Anchor Books"
        });

        let ris = csl_json_to_ris(&item);
        assert!(ris.contains("TY  - BOOK"), "should be BOOK: {ris}");
        assert!(ris.contains("PB  - Anchor Books"), "should have publisher: {ris}");
    }

    #[test]
    fn test_export_keyword_array_form_reaches_ris() {
        let item = json!({
            "type": "article-journal",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "keyword": ["ml", "nlp"]
        });
        let ris = csl_json_to_ris(&item);
        assert!(ris.contains("KW  - ml"), "array keyword → KW: {ris}");
        assert!(ris.contains("KW  - nlp"), "array keyword → KW: {ris}");
    }

    #[test]
    fn test_roundtrip_ris() {
        let input = "TY  - JOUR\nAU  - Smith, John\nTI  - Test Title\nJO  - Nature\nPY  - 2024\nVL  - 1\nSP  - 10\nEP  - 20\nDO  - 10.1234/test\nER  - ";
        let parsed = crate::parsers::ris::parse_ris(input, &crate::parsers::ParseOptions::default());
        assert_eq!(parsed.entries.len(), 1);

        let exported = csl_json_to_ris(&parsed.entries[0]);
        assert!(exported.contains("Smith"), "author should survive roundtrip: {exported}");
        assert!(exported.contains("Test Title"), "title should survive roundtrip: {exported}");
        assert!(exported.contains("2024"), "year should survive roundtrip: {exported}");
    }
}
