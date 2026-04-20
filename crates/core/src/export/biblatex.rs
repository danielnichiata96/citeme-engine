use serde_json::Value;
use super::bibtex::{disambiguate_keys, escape_bibtex, format_authors, resolve_key};

/// CSL-JSON type → BibLaTeX entry type mapping.
///
/// Key differences vs. classic BibTeX:
/// - `@online` is a valid BibLaTeX entry type for `webpage` / `post-weblog`.
/// - `@report` replaces `@techreport` (still accepted via alias).
/// - `@thesis` unifies `phdthesis` and `mastersthesis`. We emit it without a
///   `type` distinguisher; masters vs. PhD disambiguation is out of scope for
///   0.3.0 and can be added later via `item["genre"]`.
fn csl_type_to_biblatex(csl_type: &str) -> &'static str {
    match csl_type {
        "article-journal" | "article-magazine" | "article-newspaper" | "article" => "article",
        "book" => "book",
        "chapter" => "incollection",
        "paper-conference" => "inproceedings",
        "thesis" => "thesis",
        "report" => "report",
        "webpage" | "post-weblog" => "online",
        "dataset" => "dataset",
        "software" => "software",
        "patent" => "patent",
        _ => "misc",
    }
}

/// Format a date from CSL `issued.date-parts` as ISO 8601 (YYYY, YYYY-MM, YYYY-MM-DD).
///
/// BibLaTeX ≥ v3.5 canonicalized on `date` (ISO 8601); `year`/`month`/`day`
/// are now deprecated aliases. Biber requires ISO form.
fn format_biblatex_date(item: &Value) -> Option<String> {
    let parts = item["issued"]["date-parts"].as_array()?
        .first()?
        .as_array()?;
    let year = parts.first()?.as_i64()?;
    let month = parts.get(1).and_then(|m| m.as_i64());
    let day = parts.get(2).and_then(|d| d.as_i64());
    Some(match (month, day) {
        (Some(m), Some(d)) => format!("{year:04}-{m:02}-{d:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => format!("{year:04}"),
    })
}

/// Convert a CSL-JSON item to a BibLaTeX entry string.
pub fn csl_json_to_biblatex(item: &Value) -> String {
    csl_json_to_biblatex_with_key(item, &resolve_key(item))
}

pub(crate) fn csl_json_to_biblatex_with_key(item: &Value, key: &str) -> String {
    let csl_type = item["type"].as_str().unwrap_or("article-journal");
    let bib_type = csl_type_to_biblatex(csl_type);

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

    // Title (escape)
    if let Some(title) = item["title"].as_str() {
        fields.push(format!("  title = {{{}}}", escape_bibtex(title)));
    }

    // Container — `journaltitle` is canonical for articles in BibLaTeX;
    // chapters and conference papers use `booktitle`.
    if let Some(container) = item["container-title"].as_str() {
        let field_name = match bib_type {
            "article" => "journaltitle",
            "incollection" | "inproceedings" => "booktitle",
            _ => "journaltitle",
        };
        fields.push(format!("  {field_name} = {{{}}}", escape_bibtex(container)));
    }

    // Unified ISO `date`. No separate `year`/`month`/`day`.
    if let Some(d) = format_biblatex_date(item) {
        fields.push(format!("  date = {{{d}}}"));
    }

    // Volume / issue / pages
    if let Some(v) = item["volume"].as_str() { fields.push(format!("  volume = {{{v}}}")); }
    if let Some(v) = item["issue"].as_str() { fields.push(format!("  number = {{{v}}}")); }
    if let Some(v) = item["page"].as_str() { fields.push(format!("  pages = {{{v}}}")); }

    // Publisher / location (canonical; `address` is a legacy alias in BibLaTeX)
    if let Some(v) = item["publisher"].as_str() {
        fields.push(format!("  publisher = {{{}}}", escape_bibtex(v)));
    }
    if let Some(v) = item["publisher-place"].as_str() {
        fields.push(format!("  location = {{{}}}", escape_bibtex(v)));
    }

    // Identifiers
    if let Some(v) = item["DOI"].as_str() { fields.push(format!("  doi = {{{v}}}")); }
    if let Some(v) = item["URL"].as_str() { fields.push(format!("  url = {{{v}}}")); }
    if let Some(v) = item["ISBN"].as_str() { fields.push(format!("  isbn = {{{v}}}")); }
    if let Some(v) = item["ISSN"].as_str() { fields.push(format!("  issn = {{{v}}}")); }

    // Abstract / keywords / note
    if let Some(v) = item["abstract"].as_str() {
        fields.push(format!("  abstract = {{{}}}", escape_bibtex(v)));
    }
    if let Some(v) = item["keyword"].as_str() {
        fields.push(format!("  keywords = {{{}}}", escape_bibtex(v)));
    }
    if let Some(v) = item["note"].as_str() {
        fields.push(format!("  note = {{{}}}", escape_bibtex(v)));
    }

    // Series
    if let Some(v) = item["collection-title"].as_str() {
        fields.push(format!("  series = {{{}}}", escape_bibtex(v)));
    }

    // Chapter
    if let Some(v) = item["chapter-number"].as_str() {
        fields.push(format!("  chapter = {{{v}}}"));
    } else if let Some(n) = item["chapter-number"].as_i64() {
        fields.push(format!("  chapter = {{{n}}}"));
    }

    // PMID / PMCID — emit both as direct fields (top-level in CSL 1.0.1+) AND
    // as eprint/eprinttype when no `custom.eprint` is present, since the most
    // common BibLaTeX styles consult `eprint` for PubMed/arXiv linking.
    if let Some(v) = item["PMID"].as_str() { fields.push(format!("  pmid = {{{v}}}")); }
    if let Some(v) = item["PMCID"].as_str() { fields.push(format!("  pmcid = {{{v}}}")); }

    // Eprint (from CSL custom.eprint). `eprintclass` is the BibLaTeX canonical
    // (INSPIRE-HEP's `archivePrefix`/`primaryClass` is a separate convention).
    if let Some(eprint) = item["custom"]["eprint"]["id"].as_str() {
        fields.push(format!("  eprint = {{{eprint}}}"));
        if let Some(t) = item["custom"]["eprint"]["type"].as_str() {
            fields.push(format!("  eprinttype = {{{t}}}"));
        }
        if let Some(c) = item["custom"]["eprint"]["class"].as_str() {
            fields.push(format!("  eprintclass = {{{c}}}"));
        }
    } else if item["PMID"].as_str().is_some() {
        // No custom.eprint but we have PMID — emit the biblatex-idiomatic form
        // so BibLaTeX styles that link PubMed via eprint still work.
        if let Some(v) = item["PMID"].as_str() {
            fields.push(format!("  eprint = {{{v}}}"));
            fields.push("  eprinttype = {pubmed}".to_string());
        }
    }

    // Edition
    if let Some(v) = item["edition"].as_str() {
        fields.push(format!("  edition = {{{}}}", escape_bibtex(v)));
    }

    format!("@{}{{{},\n{}\n}}", bib_type, key, fields.join(",\n"))
}

/// Convert multiple CSL-JSON items to a BibLaTeX file string.
///
/// Cite keys are disambiguated with `a`/`b`/`c`/… suffixes on collision, same
/// rule as `bibtex::csl_json_array_to_bibtex`, so exporting the same array in
/// both formats yields identical keys.
pub fn csl_json_array_to_biblatex(items: &[Value]) -> String {
    let keys = disambiguate_keys(items);
    let mut out = items.iter().zip(keys.iter())
        .map(|(item, key)| csl_json_to_biblatex_with_key(item, key))
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
    fn test_export_article_uses_journaltitle() {
        let item = json!({
            "type": "article-journal",
            "id": "smith2024",
            "title": "A Study",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2024, 3, 15]]},
            "container-title": "Journal of Testing",
            "volume": "42",
            "issue": "3",
            "page": "100-115",
            "DOI": "10.1234/test"
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.starts_with("@article{smith2024,"), "{bib}");
        assert!(bib.contains("journaltitle = {Journal of Testing}"),
            "article must use journaltitle, not journal: {bib}");
        assert!(!bib.contains("\n  journal = "), "must NOT emit legacy `journal`: {bib}");
        assert!(bib.contains("date = {2024-03-15}"), "ISO 8601 full date: {bib}");
        assert!(!bib.contains("\n  year = "), "must NOT emit separate `year`: {bib}");
        assert!(!bib.contains("\n  month = "), "must NOT emit separate `month`: {bib}");
    }

    #[test]
    fn test_export_date_year_only() {
        let item = json!({
            "type": "book",
            "id": "k2024",
            "title": "T",
            "author": [{"family": "K"}],
            "issued": {"date-parts": [[2024]]}
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.contains("date = {2024}"), "year-only: {bib}");
    }

    #[test]
    fn test_export_date_year_month() {
        let item = json!({
            "type": "book",
            "id": "k2024",
            "title": "T",
            "author": [{"family": "K"}],
            "issued": {"date-parts": [[2024, 7]]}
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.contains("date = {2024-07}"), "year-month: {bib}");
    }

    #[test]
    fn test_export_book_uses_location() {
        let item = json!({
            "type": "book",
            "id": "k2014",
            "title": "T",
            "author": [{"family": "K"}],
            "issued": {"date-parts": [[2014]]},
            "publisher": "Anchor",
            "publisher-place": "New York"
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.contains("location = {New York}"),
            "must use `location`, not legacy `address`: {bib}");
        assert!(!bib.contains("\n  address = "), "no legacy address: {bib}");
    }

    #[test]
    fn test_export_webpage_uses_online() {
        let item = json!({
            "type": "webpage",
            "id": "site1",
            "title": "Page",
            "URL": "https://example.org"
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.starts_with("@online{"), "webpage → @online: {bib}");
    }

    #[test]
    fn test_export_eprint_arxiv() {
        let item = json!({
            "type": "article-journal",
            "id": "arxiv2301",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "custom": {"eprint": {"id": "2301.12345", "type": "arxiv", "class": "cs.CL"}}
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.contains("eprint = {2301.12345}"), "{bib}");
        assert!(bib.contains("eprinttype = {arxiv}"), "{bib}");
        assert!(bib.contains("eprintclass = {cs.CL}"),
            "canonical field is `eprintclass` (not primaryClass): {bib}");
    }

    #[test]
    fn test_export_pmid_dual_form() {
        // PMID emits both as `pmid = {...}` and as biblatex-idiomatic
        // `eprint = {...}, eprinttype = {pubmed}` for style compatibility.
        let item = json!({
            "type": "article-journal",
            "id": "p1",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "PMID": "38123456"
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.contains("pmid = {38123456}"), "pmid field: {bib}");
        assert!(bib.contains("eprint = {38123456}"), "eprint id: {bib}");
        assert!(bib.contains("eprinttype = {pubmed}"), "eprint type pubmed: {bib}");
    }

    #[test]
    fn test_export_pmid_when_custom_eprint_takes_priority() {
        // When both arxiv (custom.eprint) and PMID are present, arxiv wins the
        // `eprint`/`eprinttype` fields (primary identifier). PMID still emits
        // as the `pmid = {…}` direct field for roundtrip.
        let item = json!({
            "type": "article-journal",
            "id": "dual",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "PMID": "12345",
            "custom": {"eprint": {"id": "2301.0001", "type": "arxiv"}}
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.contains("pmid = {12345}"), "pmid direct field: {bib}");
        assert!(bib.contains("eprint = {2301.0001}"), "arxiv eprint wins: {bib}");
        assert!(bib.contains("eprinttype = {arxiv}"), "type stays arxiv: {bib}");
        assert!(!bib.contains("eprinttype = {pubmed}"),
            "must NOT emit pubmed eprinttype when arxiv already present: {bib}");
    }

    #[test]
    fn test_export_institutional_author() {
        let item = json!({
            "type": "report",
            "id": "who2024",
            "title": "T",
            "author": [{"literal": "World Health Organization"}],
            "issued": {"date-parts": [[2024]]}
        });
        let bib = csl_json_to_biblatex(&item);
        assert!(bib.starts_with("@report{"), "biblatex uses @report: {bib}");
        assert!(bib.contains("author = {{World Health Organization}}"),
            "literal author braced: {bib}");
    }

    #[test]
    fn test_array_dedup_keys_biblatex() {
        let items = vec![
            json!({"type": "article-journal", "author": [{"family": "Smith"}],
                   "title": "A", "issued": {"date-parts": [[2024]]}}),
            json!({"type": "article-journal", "author": [{"family": "Smith"}],
                   "title": "B", "issued": {"date-parts": [[2024]]}}),
        ];
        let out = csl_json_array_to_biblatex(&items);
        assert!(out.contains("@article{Smith2024,"), "first bare: {out}");
        assert!(out.contains("@article{Smith2024a,"), "second suffix: {out}");
    }

    #[test]
    fn test_roundtrip_bibtex_to_biblatex_preserves_metadata() {
        let input = "@article{smith2024, author = {Smith, John}, title = {T}, journal = {N}, year = {2024}, abstract = {hi}, keywords = {ml}, pmid = {12345}}";
        let parsed = crate::parsers::bibtex::parse_bibtex(input, &crate::parsers::ParseOptions::default());
        assert_eq!(parsed.entries.len(), 1);
        let exported = csl_json_to_biblatex(&parsed.entries[0]);
        assert!(exported.contains("abstract = {hi}"), "abstract roundtrips: {exported}");
        assert!(exported.contains("keywords = {ml}"), "keywords roundtrip: {exported}");
        assert!(exported.contains("pmid = {12345}"), "pmid roundtrips: {exported}");
    }
}
