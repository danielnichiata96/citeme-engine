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

/// Flatten a value into something safe for a single RIS line.
///
/// RIS is line-oriented: a bare `\n` inside a value ends the field, and a
/// crafted one (`\nER  - \nTY  - BOOK`) ends the record and opens another —
/// producing a file with more entries than items, which re-imports without a
/// single error. Nothing downstream can detect that, so newlines and tabs
/// collapse to spaces here, at the only place that writes RIS lines.
fn ris_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        let c = if matches!(c, '\r' | '\n' | '\t') {
            ' '
        } else {
            c
        };
        if c == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
        } else {
            prev_space = false;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Push one `TAG  - value` line, sanitized. Empty values are skipped so a
/// flattened-to-nothing value never emits a dangling tag.
fn push_field(lines: &mut Vec<String>, tag: &str, value: &str) {
    let v = ris_value(value);
    if !v.is_empty() {
        lines.push(format!("{tag}  - {v}"));
    }
}

/// Render one CSL-JSON author as a RIS `AU` value ("Family, Given").
///
/// Particles belong to the family name here: reading only `dropping-particle`
/// turned "Maria da Silva" into "Silva, Maria".
fn ris_author(author: &Value) -> String {
    if let Some(literal) = author["literal"].as_str() {
        return literal.to_string();
    }
    let particles = ["dropping-particle", "non-dropping-particle"]
        .iter()
        .filter_map(|k| author[*k].as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let family = author["family"].as_str().unwrap_or("").trim();
    let family = if particles.is_empty() {
        family.to_string()
    } else {
        format!("{particles} {family}").trim().to_string()
    };
    let given = author["given"].as_str().unwrap_or("").trim();
    if given.is_empty() {
        family
    } else {
        format!("{family}, {given}")
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
            push_field(&mut lines, "AU", &ris_author(author));
        }
    }

    // Title
    if let Some(v) = item["title"].as_str() {
        push_field(&mut lines, "TI", v);
    }

    // Journal / container
    if let Some(v) = item["container-title"].as_str() {
        let tag = match csl_type {
            "article-journal" | "article-magazine" | "article-newspaper" => "JO",
            _ => "T2",
        };
        push_field(&mut lines, tag, v);
    }

    // Date — RIS spec: PY for year, DA for full date (YYYY/MM/DD/ format)
    if let Some(dp) = item["issued"]["date-parts"].as_array() {
        if let Some(parts) = dp.first().and_then(|p| p.as_array()) {
            let year = parts.first().and_then(|y| y.as_i64()).unwrap_or(0);
            if year > 0 {
                lines.push(format!("PY  - {year}///"));
                let day = parts.get(2).and_then(|d| d.as_i64());
                if let Some(m) = parts.get(1).and_then(|m| m.as_i64()) {
                    let d_str = day.map(|d| format!("{d:02}")).unwrap_or_default();
                    lines.push(format!("DA  - {year}/{m:02}/{d_str}/"));
                }
            }
        }
    }

    // Volume, issue
    if let Some(v) = item["volume"].as_str() {
        push_field(&mut lines, "VL", v);
    }
    if let Some(v) = item["issue"].as_str() {
        push_field(&mut lines, "IS", v);
    }

    // Pages: split "100-115" into SP/EP
    if let Some(pages) = item["page"].as_str() {
        let parts: Vec<&str> = pages.splitn(2, ['-', '–']).collect();
        if let Some(sp) = parts.first() {
            push_field(&mut lines, "SP", sp);
        }
        if let Some(ep) = parts.get(1) {
            push_field(&mut lines, "EP", ep);
        }
    }

    // Identifiers
    if let Some(v) = item["DOI"].as_str() {
        push_field(&mut lines, "DO", v);
    }
    if let Some(v) = item["URL"].as_str() {
        push_field(&mut lines, "UR", v);
    }
    if let Some(v) = item["ISSN"].as_str() {
        push_field(&mut lines, "SN", v);
    }
    if let Some(v) = item["ISBN"].as_str() {
        push_field(&mut lines, "SN", v);
    }

    // Publisher
    if let Some(v) = item["publisher"].as_str() {
        push_field(&mut lines, "PB", v);
    }
    if let Some(v) = item["publisher-place"].as_str() {
        push_field(&mut lines, "CY", v);
    }

    // Abstract
    if let Some(v) = item["abstract"].as_str() {
        push_field(&mut lines, "AB", v);
    }

    // Language
    if let Some(v) = item["language"].as_str() {
        push_field(&mut lines, "LA", v);
    }

    // Keywords — split the normalized comma-separated form (which covers
    // both v1.0.1 string and v1.0.2 array inputs) into one `KW` tag per item.
    if let Some(kw) = super::bibtex::csl_keyword_as_string(item) {
        for k in kw.split(',') {
            let k = k.trim();
            if !k.is_empty() {
                push_field(&mut lines, "KW", k);
            }
        }
    }

    // ER (end record) — must be last
    lines.push("ER  - ".to_string());

    lines.join("\n")
}

/// Convert multiple CSL-JSON items to a RIS file string.
pub fn csl_json_array_to_ris(items: &[Value]) -> String {
    let mut out = items
        .iter()
        .map(csl_json_to_ris)
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
        assert!(
            ris.contains("AU  - Smith, John"),
            "should have author: {ris}"
        );
        assert!(
            ris.contains("AU  - Doe, Jane"),
            "should have second author: {ris}"
        );
        assert!(ris.contains("TI  - A Study"), "should have title: {ris}");
        assert!(
            ris.contains("JO  - Journal of Testing"),
            "should have journal: {ris}"
        );
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
        assert!(
            ris.contains("PB  - Anchor Books"),
            "should have publisher: {ris}"
        );
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
        let parsed =
            crate::parsers::ris::parse_ris(input, &crate::parsers::ParseOptions::default());
        assert_eq!(parsed.entries.len(), 1);

        let exported = csl_json_to_ris(&parsed.entries[0]);
        assert!(
            exported.contains("Smith"),
            "author should survive roundtrip: {exported}"
        );
        assert!(
            exported.contains("Test Title"),
            "title should survive roundtrip: {exported}"
        );
        assert!(
            exported.contains("2024"),
            "year should survive roundtrip: {exported}"
        );
    }
}
