use serde_json::Value;

/// CSL-JSON type → BibTeX entry type mapping.
/// For thesis, checks genre to distinguish phdthesis from mastersthesis.
fn csl_type_to_bibtex_with_genre(csl_type: &str, genre: Option<&str>) -> &'static str {
    match csl_type {
        "article-journal" | "article-magazine" | "article-newspaper" | "article" => "article",
        "book" => "book",
        "chapter" => "incollection",
        "paper-conference" => "inproceedings",
        "thesis" => {
            if let Some(g) = genre {
                let lower = g.to_lowercase();
                if lower.contains("master") || lower.contains("mestrado") || lower.contains("m.s.") {
                    return "mastersthesis";
                }
            }
            "phdthesis"
        }
        "report" => "techreport",
        "webpage" | "post-weblog" => "misc",
        "dataset" => "misc",
        "software" => "misc",
        "patent" => "patent",
        _ => "misc",
    }
}

/// Generate a BibTeX key from author family name and year.
fn generate_key(item: &Value) -> String {
    let author = item["author"].as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["family"].as_str().or(a["literal"].as_str()))
        .unwrap_or("unknown");

    let year = item["issued"]["date-parts"].as_array()
        .and_then(|dp| dp.first())
        .and_then(|parts| parts.as_array())
        .and_then(|parts| parts.first())
        .and_then(|y| y.as_i64())
        .map(|y| y.to_string())
        .unwrap_or_else(|| "nd".into());

    // Clean author name: remove spaces, take first word
    let clean_author: String = author.split_whitespace().next().unwrap_or(author)
        .chars().filter(|c| c.is_alphanumeric()).collect();

    format!("{}{}", clean_author, year)
}

/// Format CSL-JSON author array as BibTeX author string.
/// "Last, First and Last2, First2"
fn format_authors(authors: &[Value]) -> String {
    authors.iter()
        .map(|a| {
            if let Some(literal) = a["literal"].as_str() {
                format!("{{{literal}}}")
            } else {
                let family = a["family"].as_str().unwrap_or("");
                let given = a["given"].as_str().unwrap_or("");
                let prefix = a["dropping-particle"].as_str().unwrap_or("");
                let suffix = a["suffix"].as_str().unwrap_or("");

                let mut name = String::new();
                if !prefix.is_empty() {
                    name.push_str(prefix);
                    name.push(' ');
                }
                name.push_str(family);
                if !suffix.is_empty() {
                    name.push_str(", ");
                    name.push_str(suffix);
                }
                if !given.is_empty() {
                    name.push_str(", ");
                    name.push_str(given);
                }
                name
            }
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// Escape BibTeX special characters in field values.
fn escape_bibtex(s: &str) -> String {
    s.replace('&', r"\&")
     .replace('%', r"\%")
     .replace('$', r"\$")
     .replace('#', r"\#")
     .replace('_', r"\_")
     .replace('~', r"\textasciitilde{}")
     .replace('^', r"\textasciicircum{}")
}

/// Convert a CSL-JSON item to a BibTeX entry string.
pub fn csl_json_to_bibtex(item: &Value) -> String {
    let csl_type = item["type"].as_str().unwrap_or("article-journal");
    let genre = item["genre"].as_str();
    let bib_type = csl_type_to_bibtex_with_genre(csl_type, genre);
    let key = item["id"].as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| generate_key(item));

    let mut fields: Vec<String> = Vec::new();

    // Authors
    if let Some(authors) = item["author"].as_array() {
        if !authors.is_empty() {
            fields.push(format!("  author = {{{}}}", format_authors(authors)));
        }
    }

    // Editors
    if let Some(editors) = item["editor"].as_array() {
        if !editors.is_empty() {
            fields.push(format!("  editor = {{{}}}", format_authors(editors)));
        }
    }

    // Title (escape special chars)
    if let Some(title) = item["title"].as_str() {
        fields.push(format!("  title = {{{}}}", escape_bibtex(title)));
    }

    // Journal / booktitle / container
    if let Some(container) = item["container-title"].as_str() {
        let field_name = match bib_type {
            "article" => "journal",
            "incollection" | "inproceedings" => "booktitle",
            _ => "journal",
        };
        fields.push(format!("  {field_name} = {{{}}}", escape_bibtex(container)));
    }

    // Year
    if let Some(year) = item["issued"]["date-parts"].as_array()
        .and_then(|dp| dp.first())
        .and_then(|parts| parts.as_array())
        .and_then(|parts| parts.first())
        .and_then(|y| y.as_i64())
    {
        fields.push(format!("  year = {{{year}}}"));
    }

    // Volume, issue, pages (no escaping needed — numeric/simple values)
    if let Some(v) = item["volume"].as_str() { fields.push(format!("  volume = {{{v}}}")); }
    if let Some(v) = item["issue"].as_str() { fields.push(format!("  number = {{{v}}}")); }
    if let Some(v) = item["page"].as_str() { fields.push(format!("  pages = {{{v}}}")); }

    // Publisher (escape)
    if let Some(v) = item["publisher"].as_str() { fields.push(format!("  publisher = {{{}}}", escape_bibtex(v))); }
    if let Some(v) = item["publisher-place"].as_str() { fields.push(format!("  address = {{{}}}", escape_bibtex(v))); }

    // Identifiers (no escaping — DOI/URL/ISBN are literal)
    if let Some(v) = item["DOI"].as_str() { fields.push(format!("  doi = {{{v}}}")); }
    if let Some(v) = item["URL"].as_str() { fields.push(format!("  url = {{{v}}}")); }
    if let Some(v) = item["ISBN"].as_str() { fields.push(format!("  isbn = {{{v}}}")); }
    if let Some(v) = item["ISSN"].as_str() { fields.push(format!("  issn = {{{v}}}")); }

    // Abstract (escape)
    if let Some(v) = item["abstract"].as_str() { fields.push(format!("  abstract = {{{}}}", escape_bibtex(v))); }

    // Edition
    if let Some(v) = item["edition"].as_str() { fields.push(format!("  edition = {{{}}}", escape_bibtex(v))); }

    format!("@{}{{{},\n{}\n}}", bib_type, key, fields.join(",\n"))
}

/// Convert multiple CSL-JSON items to a BibTeX file string.
pub fn csl_json_array_to_bibtex(items: &[Value]) -> String {
    let mut out = items.iter()
        .map(|item| csl_json_to_bibtex(item))
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
    fn test_export_article() {
        let item = json!({
            "type": "article-journal",
            "id": "smith2024",
            "title": "A Study of Something",
            "author": [{"family": "Smith", "given": "John"}, {"family": "Doe", "given": "Jane"}],
            "issued": {"date-parts": [[2024]]},
            "container-title": "Journal of Testing",
            "volume": "42",
            "issue": "3",
            "page": "100-115",
            "DOI": "10.1234/test.2024"
        });

        let bib = csl_json_to_bibtex(&item);
        assert!(bib.starts_with("@article{smith2024,"), "should start with @article: {bib}");
        assert!(bib.contains("author = {Smith, John and Doe, Jane}"), "should have authors: {bib}");
        assert!(bib.contains("title = {A Study of Something}"), "should have title: {bib}");
        assert!(bib.contains("journal = {Journal of Testing}"), "should have journal: {bib}");
        assert!(bib.contains("year = {2024}"), "should have year: {bib}");
        assert!(bib.contains("doi = {10.1234/test.2024}"), "should have doi: {bib}");
    }

    #[test]
    fn test_export_book() {
        let item = json!({
            "type": "book",
            "title": "A Great Book",
            "author": [{"family": "Kwan", "given": "Kevin"}],
            "issued": {"date-parts": [[2014]]},
            "publisher": "Anchor Books"
        });

        let bib = csl_json_to_bibtex(&item);
        assert!(bib.starts_with("@book{"), "should be @book: {bib}");
        assert!(bib.contains("publisher = {Anchor Books}"));
    }

    #[test]
    fn test_export_institutional_author() {
        let item = json!({
            "type": "report",
            "title": "Report",
            "author": [{"literal": "World Health Organization"}],
            "issued": {"date-parts": [[2024]]}
        });

        let bib = csl_json_to_bibtex(&item);
        assert!(bib.contains("author = {{World Health Organization}}"), "literal author should be braced: {bib}");
    }

    #[test]
    fn test_roundtrip_bibtex() {
        // Parse real BibTeX, export back to BibTeX, verify key fields survive
        let input = "@article{test2024, author = {Smith, John}, title = {Test Title}, journal = {Nature}, year = {2024}, volume = {1}, pages = {10-20}, doi = {10.1234/test}}";
        let parsed = crate::parsers::bibtex::parse_bibtex(input, &crate::parsers::ParseOptions::default());
        assert_eq!(parsed.entries.len(), 1);

        let exported = csl_json_to_bibtex(&parsed.entries[0]);
        assert!(exported.contains("Smith"), "author should survive roundtrip: {exported}");
        assert!(exported.contains("Test Title"), "title should survive roundtrip: {exported}");
        assert!(exported.contains("2024"), "year should survive roundtrip: {exported}");
    }
}
