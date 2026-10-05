use super::{date_parts, text_field, unique_keys};
use serde_json::Value;

/// CSL-JSON type → Hayagriva entry type mapping.
fn csl_type_to_hayagriva(csl_type: &str) -> &'static str {
    match csl_type {
        "article-journal" | "article-magazine" | "article-newspaper" | "article" => "Article",
        "book" => "Book",
        "chapter" => "Chapter",
        "paper-conference" => "Article",
        "thesis" => "Thesis",
        "report" => "Report",
        "webpage" | "post-weblog" => "Web",
        "dataset" => "Repository",
        "software" => "Repository",
        _ => "Article",
    }
}

/// One person as a Hayagriva YAML list item, in dictionary form: `name`
/// (the family name), `given-name`, `prefix`, `suffix`.
///
/// hayagriva reads a bare string as "Family, Given[, Suffix]", so the
/// "Given Family" strings this used to write came back wrong: "John Smith"
/// as a family name with no given name, "Maria da Silva" as family "Silva"
/// with prefix "Maria da", and a literal with three commas failed the whole
/// file. A literal is the `name` alone; a given name with no family name is
/// a mononym, which hayagriva keeps in `name` too. `None` for a nameless
/// person.
fn person_yaml(person: &Value) -> Option<String> {
    let part = |key: &str| {
        person[key]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    if let Some(literal) = part("literal") {
        return Some(format!("    - name: {}", yaml_str(literal)));
    }
    let (name, given) = match (part("family"), part("given")) {
        (Some(family), given) => (family, given),
        (None, Some(given)) => (given, None),
        (None, None) => return None,
    };
    let mut lines = vec![format!("    - name: {}", yaml_str(name))];
    if let Some(given) = given {
        lines.push(format!("      given-name: {}", yaml_str(given)));
    }
    // Both particle kinds go in `prefix`, in CSL name order. Reading only
    // `dropping-particle` turned "Maria da Silva" into "Maria Silva".
    let prefix = ["dropping-particle", "non-dropping-particle"]
        .iter()
        .filter_map(|k| part(k))
        .collect::<Vec<_>>()
        .join(" ");
    if !prefix.is_empty() {
        lines.push(format!("      prefix: {}", yaml_str(&prefix)));
    }
    if let Some(suffix) = part("suffix") {
        lines.push(format!("      suffix: {}", yaml_str(suffix)));
    }
    Some(lines.join("\n"))
}

/// A `persons:` block, or nothing when no person has a name.
fn push_persons(lines: &mut Vec<String>, field: &str, persons: &Value) {
    let Some(persons) = persons.as_array() else {
        return;
    };
    let items: Vec<String> = persons.iter().filter_map(person_yaml).collect();
    if !items.is_empty() {
        lines.push(format!("  {field}:"));
        lines.extend(items);
    }
}

/// `date:` as hayagriva reads it: a four-digit year, signed when negative
/// (`0800-03`, `-0350`) — `800-03` failed the whole file — and a day only
/// when its month has it (`date_parts` sees to that). A year past four
/// digits only loads bare, so it goes without month and day.
fn hayagriva_date(item: &Value) -> Option<String> {
    let date = date_parts(item, "issued")?;
    Some(date.iso().unwrap_or_else(|| date.year.to_string()))
}

/// Whether hayagriva reads `url` as a URL: it parses an absolute URL, so
/// "www.example.org/page" or "/papers/1.pdf" failed the whole file. Checked
/// with hayagriva's own parser, so the answer is the loader's.
fn is_hayagriva_url(url: &str) -> bool {
    url.parse::<hayagriva::types::QualifiedUrl>().is_ok()
}

/// Escape a YAML string value.
///
/// Whitelist, not blacklist: a plain (unquoted) scalar is only emitted when
/// it starts with an alphanumeric char, contains nothing outside a small
/// safe set, and cannot be type-coerced by YAML (`true`, `null`, numbers…).
/// Everything else is double-quoted with full escape coverage. The previous
/// blacklist missed `true`/`null`, leading `-`/`@`/`*`/`&`/`!`, embedded
/// `: ` and more — each of those silently produced YAML that hayagriva's
/// own reader rejects.
fn yaml_str(s: &str) -> String {
    if is_plain_yaml_safe(s) {
        s.to_string()
    } else {
        yaml_quote(s)
    }
}

fn is_plain_yaml_safe(s: &str) -> bool {
    if s.is_empty() || s.ends_with(' ') {
        return false;
    }
    // Scalars YAML 1.1/1.2 loaders may coerce to bool/null/number.
    let lower = s.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
    ) {
        return false;
    }
    if s.parse::<f64>().is_ok() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphanumeric() {
        return false;
    }
    s.chars().all(|c| {
        c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '/' | '(' | ')' | '+' | ';')
    })
}

/// Double-quoted YAML scalar with escapes for backslash, quote, control
/// chars and newlines (a raw newline inside a double-quoted scalar folds —
/// content would silently change). U+FFFE and U+FFFF are escaped too:
/// libyaml refuses them anywhere in the stream, so one of them in one title
/// failed the whole file.
fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{FFFE}' | '\u{FFFF}') => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `language` maps to hayagriva's `LanguageIdentifier` (BCP-47) — junk
/// values fail the YAML load even when quoted, so anything that doesn't
/// look like a language tag is omitted entirely rather than emitted.
fn is_language_tag(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 35
        && s.split('-').enumerate().all(|(i, part)| {
            let len_ok = if i == 0 {
                (2..=3).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphabetic())
            } else {
                (1..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
            };
            len_ok
        })
}

/// Resolve the YAML mapping key for an item: the sanitized `id` (a number
/// is an id too), or `entry` when the id is absent, empty, or sanitizes to
/// nothing.
///
/// A key must never be empty — `"": {…}` is not a usable entry — and callers
/// exporting more than one item must disambiguate, because duplicate YAML
/// keys mean the last one silently wins and the earlier items vanish on load.
fn resolve_hayagriva_key(item: &Value) -> String {
    let sanitized: String = text_field(item, "id")
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.chars().any(|c| c.is_alphanumeric()) {
        sanitized
    } else {
        "entry".to_string()
    }
}

/// Convert a CSL-JSON item to a Hayagriva YAML entry string.
pub fn csl_json_to_hayagriva(item: &Value) -> String {
    csl_json_to_hayagriva_with_key(item, &resolve_hayagriva_key(item))
}

/// Convert a CSL-JSON item to a Hayagriva YAML entry string under an explicit
/// key. Used by the array exporter to emit disambiguated keys.
pub(crate) fn csl_json_to_hayagriva_with_key(item: &Value, key: &str) -> String {
    let csl_type = item["type"].as_str().unwrap_or("article-journal");
    let hay_type = csl_type_to_hayagriva(csl_type);

    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("{key}:"));
    lines.push(format!("  type: {hay_type}"));

    // Title
    if let Some(title) = item["title"].as_str() {
        lines.push(format!("  title: {}", yaml_str(title)));
    }

    push_persons(&mut lines, "author", &item["author"]);
    push_persons(&mut lines, "editor", &item["editor"]);

    if let Some(date) = hayagriva_date(item) {
        lines.push(format!("  date: {date}"));
    }

    // Parent (container/journal)
    if let Some(container) = item["container-title"].as_str() {
        lines.push("  parent:".to_string());
        lines.push(format!("    - title: {}", yaml_str(container)));
        // Add parent type based on CSL type
        let parent_type = match csl_type {
            "article-journal" | "article-magazine" => "Periodical",
            "chapter" => "Book",
            "paper-conference" => "Proceedings",
            _ => "Periodical",
        };
        lines.push(format!("      type: {parent_type}"));
    }

    // Biblio fields
    if let Some(v) = text_field(item, "volume") {
        lines.push(format!("  volume: {}", yaml_str(&v)));
    }
    if let Some(v) = text_field(item, "issue") {
        lines.push(format!("  issue: {}", yaml_str(&v)));
    }
    if let Some(v) = text_field(item, "page") {
        lines.push(format!("  page-range: {}", yaml_str(&v)));
    }
    if let Some(v) = text_field(item, "edition") {
        lines.push(format!("  edition: {}", yaml_str(&v)));
    }

    // Publisher
    if let Some(pub_name) = item["publisher"].as_str() {
        lines.push("  publisher:".to_string());
        lines.push(format!("    name: {}", yaml_str(pub_name)));
        if let Some(loc) = item["publisher-place"].as_str() {
            lines.push(format!("    location: {}", yaml_str(loc)));
        }
    }

    // Serial-number (identifiers nested under serial-number:), string or
    // number. CSL `number` (a report or patent number) is the `serial` key,
    // where hayagriva's CSL renderer looks for it.
    let serial_fields: Vec<String> = [
        ("DOI", "doi"),
        ("ISBN", "isbn"),
        ("ISSN", "issn"),
        ("PMID", "pmid"),
        ("PMCID", "pmcid"),
        ("number", "serial"),
    ]
    .iter()
    .filter_map(|(csl, key)| text_field(item, csl).map(|v| format!("    {key}: {}", yaml_str(&v))))
    .collect();
    if !serial_fields.is_empty() {
        lines.push("  serial-number:".to_string());
        lines.extend(serial_fields);
    }

    // URL (separate from serial-number) — only one hayagriva can parse.
    if let Some(v) = item["URL"].as_str().filter(|v| is_hayagriva_url(v)) {
        lines.push(format!("  url: {}", yaml_str(v)));
    }

    // Genre
    if let Some(v) = item["genre"].as_str() {
        lines.push(format!("  genre: {}", yaml_str(v)));
    }

    // Chapter
    if let Some(v) = text_field(item, "chapter-number") {
        lines.push(format!("  chapter: {}", yaml_str(&v)));
    }

    // Language — validated, never quoted-junk (see is_language_tag)
    if let Some(v) = item["language"].as_str() {
        if is_language_tag(v) {
            lines.push(format!("  language: {v}"));
        }
    }

    // Abstract
    if let Some(v) = item["abstract"].as_str() {
        lines.push(format!("  abstract: {}", yaml_str(v)));
    }

    lines.join("\n")
}

/// Convert multiple CSL-JSON items to a Hayagriva YAML file string.
pub fn csl_json_array_to_hayagriva(items: &[Value]) -> String {
    // Keys must be unique across the file: items with no `id` all resolved to
    // the literal `entry`, so exporting three of them produced one entry and
    // silently dropped two on load. Repeats become `entry-2`, `entry-3`, …
    let keys = unique_keys(
        items.iter().map(resolve_hayagriva_key).collect(),
        |base, n| format!("{base}-{}", n + 1),
        str::to_string,
    );
    let mut out = items
        .iter()
        .zip(&keys)
        .map(|(item, key)| csl_json_to_hayagriva_with_key(item, key))
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
    fn test_export_hayagriva_article() {
        let item = json!({
            "type": "article-journal",
            "id": "smith2024",
            "title": "A Study of Something",
            "author": [{"family": "Smith", "given": "John"}, {"family": "Doe", "given": "Jane"}],
            "issued": {"date-parts": [[2024, 3, 15]]},
            "container-title": "Journal of Testing",
            "volume": "42",
            "DOI": "10.1234/test"
        });

        let yaml = csl_json_to_hayagriva(&item);
        assert!(
            yaml.starts_with("smith2024:"),
            "should start with key: {yaml}"
        );
        assert!(yaml.contains("type: Article"), "should have type: {yaml}");
        assert!(
            yaml.contains("title: A Study of Something"),
            "should have title: {yaml}"
        );
        assert!(
            yaml.contains("    - name: Smith\n      given-name: John\n"),
            "should have author in dictionary form: {yaml}"
        );
        assert!(
            yaml.contains("    - name: Doe\n      given-name: Jane\n"),
            "should have second author: {yaml}"
        );
        assert!(
            yaml.contains("date: 2024-03-15"),
            "should have full date: {yaml}"
        );
        assert!(
            yaml.contains("title: Journal of Testing"),
            "should have parent: {yaml}"
        );
        assert!(
            yaml.contains("type: Periodical"),
            "parent should be Periodical: {yaml}"
        );
        assert!(
            yaml.contains("volume: \"42\""),
            "should have volume: {yaml}"
        );
        assert!(
            yaml.contains("    doi: 10.1234/test"),
            "should have doi under serial-number: {yaml}"
        );
    }

    #[test]
    fn test_export_hayagriva_book() {
        let item = json!({
            "type": "book",
            "id": "kwan2014",
            "title": "Crazy Rich Asians",
            "author": [{"family": "Kwan", "given": "Kevin"}],
            "issued": {"date-parts": [[2014]]},
            "publisher": "Anchor Books",
            "publisher-place": "New York"
        });

        let yaml = csl_json_to_hayagriva(&item);
        assert!(yaml.contains("type: Book"), "should be Book: {yaml}");
        assert!(
            yaml.contains("name: Anchor Books"),
            "should have publisher: {yaml}"
        );
        assert!(
            yaml.contains("location: New York"),
            "should have location: {yaml}"
        );
        assert!(
            yaml.contains("date: 2014"),
            "should have year-only date: {yaml}"
        );
    }

    #[test]
    fn test_export_hayagriva_special_chars() {
        let item = json!({
            "type": "article-journal",
            "id": "test1",
            "title": "Title with: colon and \"quotes\"",
            "author": [{"family": "Smith", "given": "J."}],
            "issued": {"date-parts": [[2024]]}
        });

        let yaml = csl_json_to_hayagriva(&item);
        assert!(
            yaml.contains("\"Title with: colon and \\\"quotes\\\"\""),
            "special chars should be escaped: {yaml}"
        );
    }

    #[test]
    fn test_export_hayagriva_serial_number() {
        let item = json!({
            "type": "article-journal",
            "id": "smith2024",
            "title": "A Study",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2024]]},
            "DOI": "10.1234/test",
            "ISBN": "978-0747551003",
            "ISSN": "2412-3129",
            "PMID": "12345678",
            "PMCID": "PMC9876543"
        });

        let yaml = csl_json_to_hayagriva(&item);
        // Identifiers MUST be nested under serial-number:
        assert!(
            yaml.contains("  serial-number:"),
            "should have serial-number block: {yaml}"
        );
        assert!(
            yaml.contains("    doi: 10.1234/test"),
            "doi nested under serial-number: {yaml}"
        );
        assert!(
            yaml.contains("    isbn: 978-0747551003"),
            "isbn nested under serial-number: {yaml}"
        );
        assert!(
            yaml.contains("    issn: 2412-3129"),
            "issn nested under serial-number: {yaml}"
        );
        assert!(
            yaml.contains("    pmid: \"12345678\""),
            "pmid should be quoted (numeric): {yaml}"
        );
        assert!(
            yaml.contains("    pmcid: PMC9876543"),
            "pmcid nested under serial-number: {yaml}"
        );
        // Must NOT have flat doi/isbn/issn fields at root level
        assert!(!yaml.contains("\n  doi:"), "doi must not be flat: {yaml}");
        assert!(!yaml.contains("\n  isbn:"), "isbn must not be flat: {yaml}");
        assert!(!yaml.contains("\n  issn:"), "issn must not be flat: {yaml}");
    }

    #[test]
    fn test_export_hayagriva_genre_and_chapter() {
        let item = json!({
            "type": "thesis",
            "id": "lee2024",
            "title": "My Dissertation",
            "author": [{"family": "Lee", "given": "Alice"}],
            "issued": {"date-parts": [[2024]]},
            "genre": "Doctoral dissertation",
            "chapter-number": "3"
        });

        let yaml = csl_json_to_hayagriva(&item);
        assert!(
            yaml.contains("  genre: Doctoral dissertation"),
            "should have genre: {yaml}"
        );
        assert!(
            yaml.contains("  chapter: \"3\""),
            "should have chapter (quoted numeric): {yaml}"
        );
    }

    #[test]
    fn test_yaml_quote_escapes_what_libyaml_refuses() {
        assert_eq!(yaml_quote("a\u{FFFE}b\u{FFFF}"), "\"a\\uFFFEb\\uFFFF\"");
        assert_eq!(yaml_quote("a\u{7F}\u{85}"), "\"a\\u007F\\u0085\"");
        // Everything else libyaml reads as written.
        assert_eq!(yaml_quote("a\u{FEFF}\u{2028}😀"), "\"a\u{FEFF}\u{2028}😀\"");
    }

    #[test]
    fn test_person_yaml_dictionary_form() {
        let lines = |v: Value| person_yaml(&v);
        assert_eq!(
            lines(json!({"family": "Silva", "given": "Maria",
                "dropping-particle": "de", "non-dropping-particle": "da", "suffix": "Neto"})),
            Some(
                "    - name: Silva\n      given-name: Maria\n      prefix: de da\n      suffix: Neto"
                    .into()
            )
        );
        assert_eq!(
            lines(json!({"literal": "IBGE, Rio"})),
            Some("    - name: \"IBGE, Rio\"".into())
        );
        assert_eq!(
            lines(json!({"given": "Plato"})),
            Some("    - name: Plato".into())
        );
        assert_eq!(lines(json!({"family": " ", "given": ""})), None);
    }

    #[test]
    fn test_yaml_str_quotes_numeric_strings() {
        // Pure numeric strings must be quoted to prevent YAML interpreting as int/float
        assert_eq!(yaml_str("12345678"), "\"12345678\"");
        assert_eq!(yaml_str("3.14"), "\"3.14\"");
        assert_eq!(yaml_str("0"), "\"0\"");
        // Non-numeric strings should remain unquoted
        assert_eq!(yaml_str("PMC9876543"), "PMC9876543");
        assert_eq!(yaml_str("10.1234/test"), "10.1234/test");
        assert_eq!(yaml_str("978-0747551003"), "978-0747551003");
    }
}
