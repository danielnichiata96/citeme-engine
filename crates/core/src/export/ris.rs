use super::{date_parts, text_field, DateParts};
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
        "patent" => "PAT",
        _ => "JOUR",
    }
}

/// A date in RIS form, `YYYY/MM/DD/` — empty parts stay empty.
fn ris_date(date: &DateParts) -> String {
    let month = date.month.map(|m| format!("{m:02}")).unwrap_or_default();
    let day = date.day.map(|d| format!("{d:02}")).unwrap_or_default();
    format!("{}/{month}/{day}/", date.year)
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

    // Editors. For a chapter or a proceedings paper they edit the host book,
    // which RIS records as the secondary author `A2`; everywhere else `ED`.
    // Our importer reads both back as `editor`.
    if let Some(editors) = item["editor"].as_array() {
        let tag = match csl_type {
            "chapter" | "paper-conference" => "A2",
            _ => "ED",
        };
        for editor in editors {
            push_field(&mut lines, tag, &ris_author(editor));
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
    if let Some(date) = date_parts(item, "issued").filter(|d| d.year > 0) {
        lines.push(format!("PY  - {}///", date.year));
        if date.month.is_some() {
            lines.push(format!("DA  - {}", ris_date(&date)));
        }
    }

    // Access date — `Y2`, as Zotero and EndNote write it for every type.
    if let Some(date) = date_parts(item, "accessed").filter(|d| d.year > 0) {
        lines.push(format!("Y2  - {}", ris_date(&date)));
    }

    // Volume, issue
    if let Some(v) = text_field(item, "volume") {
        push_field(&mut lines, "VL", &v);
    }
    if let Some(v) = text_field(item, "issue") {
        push_field(&mut lines, "IS", &v);
    }

    // CSL `number`: a journal article's article number is `C7`; any other
    // type's number (report, patent, thesis, …) is the generic `M1`. Not
    // `SN`, which importers — ours included — read as an ISBN or ISSN.
    if let Some(v) = text_field(item, "number") {
        let tag = match csl_type_to_ris(csl_type) {
            "JOUR" => "C7",
            _ => "M1",
        };
        push_field(&mut lines, tag, &v);
    }

    // Pages: split "100-115" into SP/EP
    if let Some(pages) = text_field(item, "page") {
        let parts: Vec<&str> = pages.splitn(2, ['-', '–']).collect();
        if let Some(sp) = parts.first() {
            push_field(&mut lines, "SP", sp);
        }
        if let Some(ep) = parts.get(1) {
            push_field(&mut lines, "EP", ep);
        }
    }

    // Identifiers — string or number, as CSL-JSON allows (`"ISBN":
    // 9780306406157` was dropped by an `as_str` read).
    for (key, tag) in [("DOI", "DO"), ("URL", "UR"), ("ISSN", "SN"), ("ISBN", "SN")] {
        if let Some(v) = text_field(item, key) {
            push_field(&mut lines, tag, &v);
        }
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

    // Keywords — one `KW` tag each. A v1.0.2 list already holds them one by
    // one, and an item may contain commas (MeSH: "Carcinoma, Non-Small-Cell
    // Lung"), so only the v1.0.1 comma-separated string is split.
    let keywords: Vec<String> = match &item["keyword"] {
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Value::String(s) => s.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    };
    for k in &keywords {
        push_field(&mut lines, "KW", k);
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
