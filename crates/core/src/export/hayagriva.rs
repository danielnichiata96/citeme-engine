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

/// Format an author for Hayagriva YAML.
/// Hayagriva format: "Given Family" or just "Literal Name"
fn format_author_yaml(author: &Value) -> String {
    if let Some(literal) = author["literal"].as_str() {
        return literal.to_string();
    }
    let family = author["family"].as_str().unwrap_or("");
    let given = author["given"].as_str().unwrap_or("");
    let prefix = author["dropping-particle"].as_str().unwrap_or("");

    let mut name = String::new();
    if !given.is_empty() {
        name.push_str(given);
        name.push(' ');
    }
    if !prefix.is_empty() {
        name.push_str(prefix);
        name.push(' ');
    }
    name.push_str(family);
    name.trim().to_string()
}

/// Escape a YAML string value. Wraps in quotes if it contains special chars.
fn yaml_str(s: &str) -> String {
    if s.contains(':') || s.contains('#') || s.contains('"') || s.contains('\'')
        || s.contains('\n') || s.starts_with('{') || s.starts_with('[')
        || s.starts_with(' ') || s.ends_with(' ')
    {
        // Use double quotes with escaped internal quotes
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Convert a CSL-JSON item to a Hayagriva YAML entry string.
pub fn csl_json_to_hayagriva(item: &Value) -> String {
    let csl_type = item["type"].as_str().unwrap_or("article-journal");
    let hay_type = csl_type_to_hayagriva(csl_type);

    let key = item["id"].as_str()
        .unwrap_or("entry");

    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("{key}:"));
    lines.push(format!("  type: {hay_type}"));

    // Title
    if let Some(title) = item["title"].as_str() {
        lines.push(format!("  title: {}", yaml_str(title)));
    }

    // Authors
    if let Some(authors) = item["author"].as_array() {
        if !authors.is_empty() {
            lines.push("  author:".to_string());
            for author in authors {
                lines.push(format!("    - {}", yaml_str(&format_author_yaml(author))));
            }
        }
    }

    // Editors
    if let Some(editors) = item["editor"].as_array() {
        if !editors.is_empty() {
            lines.push("  editor:".to_string());
            for editor in editors {
                lines.push(format!("    - {}", yaml_str(&format_author_yaml(editor))));
            }
        }
    }

    // Date
    if let Some(dp) = item["issued"]["date-parts"].as_array() {
        if let Some(parts) = dp.first().and_then(|p| p.as_array()) {
            let year = parts.first().and_then(|y| y.as_i64()).unwrap_or(0);
            if year > 0 {
                let month = parts.get(1).and_then(|m| m.as_i64());
                let day = parts.get(2).and_then(|d| d.as_i64());
                let date_str = match (month, day) {
                    (Some(m), Some(d)) => format!("{year}-{m:02}-{d:02}"),
                    (Some(m), None) => format!("{year}-{m:02}"),
                    _ => format!("{year}"),
                };
                lines.push(format!("  date: {date_str}"));
            }
        }
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
    if let Some(v) = item["volume"].as_str() { lines.push(format!("  volume: {v}")); }
    if let Some(v) = item["issue"].as_str() { lines.push(format!("  issue: {v}")); }
    if let Some(v) = item["page"].as_str() { lines.push(format!("  page-range: {v}")); }
    if let Some(v) = item["edition"].as_str() { lines.push(format!("  edition: {v}")); }

    // Publisher
    if let Some(pub_name) = item["publisher"].as_str() {
        lines.push("  publisher:".to_string());
        lines.push(format!("    name: {}", yaml_str(pub_name)));
        if let Some(loc) = item["publisher-place"].as_str() {
            lines.push(format!("    location: {}", yaml_str(loc)));
        }
    }

    // Identifiers
    if let Some(v) = item["DOI"].as_str() { lines.push(format!("  doi: {}", yaml_str(v))); }
    if let Some(v) = item["URL"].as_str() { lines.push(format!("  url: {}", yaml_str(v))); }
    if let Some(v) = item["ISBN"].as_str() { lines.push(format!("  isbn: {}", yaml_str(v))); }
    if let Some(v) = item["ISSN"].as_str() { lines.push(format!("  issn: {}", yaml_str(v))); }

    // Language
    if let Some(v) = item["language"].as_str() { lines.push(format!("  language: {v}")); }

    // Abstract
    if let Some(v) = item["abstract"].as_str() {
        lines.push(format!("  abstract: {}", yaml_str(v)));
    }

    lines.join("\n")
}

/// Convert multiple CSL-JSON items to a Hayagriva YAML file string.
pub fn csl_json_array_to_hayagriva(items: &[Value]) -> String {
    items.iter()
        .map(|item| csl_json_to_hayagriva(item))
        .collect::<Vec<_>>()
        .join("\n\n")
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
        assert!(yaml.starts_with("smith2024:"), "should start with key: {yaml}");
        assert!(yaml.contains("type: Article"), "should have type: {yaml}");
        assert!(yaml.contains("title: A Study of Something"), "should have title: {yaml}");
        assert!(yaml.contains("- John Smith"), "should have author: {yaml}");
        assert!(yaml.contains("- Jane Doe"), "should have second author: {yaml}");
        assert!(yaml.contains("date: 2024-03-15"), "should have full date: {yaml}");
        assert!(yaml.contains("title: Journal of Testing"), "should have parent: {yaml}");
        assert!(yaml.contains("type: Periodical"), "parent should be Periodical: {yaml}");
        assert!(yaml.contains("volume: 42"), "should have volume: {yaml}");
        assert!(yaml.contains("doi: 10.1234/test"), "should have doi: {yaml}");
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
        assert!(yaml.contains("name: Anchor Books"), "should have publisher: {yaml}");
        assert!(yaml.contains("location: New York"), "should have location: {yaml}");
        assert!(yaml.contains("date: 2014"), "should have year-only date: {yaml}");
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
        assert!(yaml.contains("\"Title with: colon and \\\"quotes\\\"\""),
            "special chars should be escaped: {yaml}");
    }
}
