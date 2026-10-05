use super::{date_parts, iso_date, text_field, unique_keys};
use serde_json::Value;

/// Whether a thesis genre names a master's degree ("Master's thesis",
/// "Dissertação (Mestrado)", MSc, maestría). In Portuguese a *dissertação*
/// is the master's work and a *tese* the doctoral one.
fn is_masters_thesis(genre: Option<&str>) -> bool {
    genre.is_some_and(|g| {
        let lower = g.to_lowercase();
        [
            "master",
            "mestrado",
            "m.s.",
            "msc",
            "m.sc",
            "maestr",
            "dissertação",
            "dissertacao",
        ]
        .iter()
        .any(|n| lower.contains(n))
    })
}

/// CSL-JSON type → BibTeX entry type mapping.
/// For thesis, checks genre to distinguish phdthesis from mastersthesis.
fn csl_type_to_bibtex_with_genre(csl_type: &str, genre: Option<&str>) -> &'static str {
    match csl_type {
        "article-journal" | "article-magazine" | "article-newspaper" | "article" => "article",
        "book" => "book",
        "chapter" => "incollection",
        "paper-conference" => "inproceedings",
        "thesis" if is_masters_thesis(genre) => "mastersthesis",
        "thesis" => "phdthesis",
        "report" => "techreport",
        "webpage" | "post-weblog" => "misc",
        "dataset" => "misc",
        "software" => "misc",
        "patent" => "patent",
        _ => "misc",
    }
}

/// Generate a BibTeX key from author family name and year.
pub(crate) fn generate_key(item: &Value) -> String {
    let author = item["author"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["family"].as_str().or(a["literal"].as_str()))
        .unwrap_or("unknown");

    let year = date_parts(item, "issued")
        .map(|d| d.year.to_string())
        .unwrap_or_else(|| "nd".into());

    // Clean author name: remove spaces, take first word
    let clean_author: String = author
        .split_whitespace()
        .next()
        .unwrap_or(author)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();

    format!("{}{}", clean_author, year)
}

/// Escape one part of a name, bracing it when BibTeX would read it as
/// structure: a comma splits a name into family/suffix/given, and the word
/// "and" (any case) splits the list into names. Bare, "Procter and Gamble"
/// re-imported as two authors and the family "Smith, Jones" as "Smith"
/// with a suffix.
fn name_part(part: &str) -> String {
    let escaped = escape_bibtex(part);
    let structural = part.contains(',')
        || part
            .split_whitespace()
            .any(|word| word.eq_ignore_ascii_case("and"));
    if structural {
        format!("{{{escaped}}}")
    } else {
        escaped
    }
}

/// Format CSL-JSON author array as BibTeX author string.
/// "Last, First and Last2, First2"
pub(crate) fn format_authors(authors: &[Value]) -> String {
    authors
        .iter()
        .map(|a| {
            if let Some(literal) = a["literal"].as_str() {
                format!("{{{}}}", escape_bibtex(literal))
            } else {
                // Every part is escaped: an unescaped `}` in a family name used
                // to close the field and let the rest of the value open a
                // second entry, corrupting the file without raising anything.
                let family = name_part(a["family"].as_str().unwrap_or(""));
                let given = name_part(a["given"].as_str().unwrap_or(""));
                let suffix = name_part(a["suffix"].as_str().unwrap_or(""));
                // CSL name order is: given · dropping · non-dropping · family.
                // Reading only `dropping-particle` silently lost the "da" in
                // "Maria da Silva" — the form CiteMe actually emits.
                let prefix = ["dropping-particle", "non-dropping-particle"]
                    .iter()
                    .filter_map(|k| a[*k].as_str())
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(name_part)
                    .collect::<Vec<_>>()
                    .join(" ");

                let mut name = String::new();
                if !prefix.is_empty() {
                    name.push_str(&prefix);
                    name.push(' ');
                }
                name.push_str(&family);
                if !suffix.is_empty() {
                    name.push_str(", ");
                    name.push_str(&suffix);
                }
                if !given.is_empty() {
                    name.push_str(", ");
                    name.push_str(&given);
                }
                name
            }
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// Read a CSL `keyword` as a normalized comma-separated string, tolerating
/// both the v1.0.1 string form and the v1.0.2 array form.
///
/// CSL v1.0.1 defined `keyword` as a single string (comma-separated by
/// convention). v1.0.2 also permits an array of strings. Consumers emit
/// either form in the wild; this helper collapses both to the string shape
/// our exporters already emit, so an array-form input isn't silently dropped.
pub(crate) fn csl_keyword_as_string(item: &Value) -> Option<String> {
    if let Some(s) = item["keyword"].as_str() {
        let trimmed = s.trim();
        return if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    if let Some(arr) = item["keyword"].as_array() {
        let joined: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        return if joined.is_empty() {
            None
        } else {
            Some(joined.join(", "))
        };
    }
    None
}

/// Escape BibTeX special characters in field values.
///
/// Single-pass over chars — chained `.replace()` would re-scan backslashes
/// introduced by earlier replacements. Braces and backslash MUST be covered:
/// an unbalanced `{` in a title makes the whole entry (and every entry after
/// it) unparseable.
pub(crate) fn escape_bibtex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\textbackslash{}"),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            '&' => out.push_str(r"\&"),
            '%' => out.push_str(r"\%"),
            '$' => out.push_str(r"\$"),
            '#' => out.push_str(r"\#"),
            '_' => out.push_str(r"\_"),
            '~' => out.push_str(r"\textasciitilde{}"),
            '^' => out.push_str(r"\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}

/// Make a value safe for a verbatim field (`doi`, `url`, `eprint`).
///
/// Verbatim fields are not TeX-escaped — `\_` in a URL would be a literal
/// backslash — but their braces must still balance or the field closes
/// early. Braces and backslashes are percent-encoded instead, which URL and
/// DOI resolvers read back to the same characters.
pub(crate) fn escape_verbatim(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '{' => out.push_str("%7B"),
            '}' => out.push_str("%7D"),
            '\\' => out.push_str("%5C"),
            _ => out.push(c),
        }
    }
    out
}

/// Sanitize a raw id into a legal BibTeX cite key. Commas, braces, parens,
/// whitespace, `=`, `\`, `#`, `%`, `"` and `~` all break entry syntax — they
/// become `_`. Alphanumerics plus `- _ : . / +` pass through (covers
/// DOI-shaped ids like `id:10.1000/xyz`).
fn sanitize_key(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.' | '/' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Resolve a cite key from a CSL-JSON item: use explicit `id` if present
/// (sanitized), else derive one via `generate_key`. Ids with no alphanumeric
/// content at all fall back to the derived key. A numeric id (valid
/// CSL-JSON) is a key like any other — read as a string only, `"id": 7`
/// lost its identity to the author-year key. Callers that need dedup across
/// an array should use `csl_json_to_bibtex_with_key` with their own key.
pub(crate) fn resolve_key(item: &Value) -> String {
    text_field(item, "id")
        .map(|id| sanitize_key(&id))
        .filter(|k| k.chars().any(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| generate_key(item))
}

/// BibTeX has one `number` field: an article's issue, but a report's or a
/// patent's own number. An article's CSL `number` is its article number,
/// which BibTeX and BibLaTeX styles read from `eid`. Returns the values for
/// `number` and `eid`; a non-article with both an issue and a number keeps
/// the number, the one its entry type means.
pub(crate) fn number_and_eid(item: &Value, is_article: bool) -> (Option<String>, Option<String>) {
    let issue = text_field(item, "issue");
    let number = text_field(item, "number");
    if is_article {
        (issue, number)
    } else {
        (number.or(issue), None)
    }
}

/// Convert a CSL-JSON item to a BibTeX entry string.
pub fn csl_json_to_bibtex(item: &Value) -> String {
    csl_json_to_bibtex_with_key(item, &resolve_key(item))
}

/// Convert a CSL-JSON item to a BibTeX entry string with an explicit cite key.
///
/// This is the primitive used by `csl_json_array_to_bibtex` to emit entries
/// with a/b/c suffixes when multiple items resolve to the same base key.
pub(crate) fn csl_json_to_bibtex_with_key(item: &Value, key: &str) -> String {
    let csl_type = item["type"].as_str().unwrap_or("article-journal");
    let genre = item["genre"].as_str();
    let bib_type = csl_type_to_bibtex_with_genre(csl_type, genre);

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

    // Year + month. BibTeX tradition: `month = mar` (3-letter lowercase macro,
    // no braces). Never numeric. Day is not a canonical BibTeX field. A year
    // past four digits is left out: `year = {20240}` makes biblatex reject
    // the whole entry.
    if let Some(date) = date_parts(item, "issued").filter(|d| d.iso_year().is_some()) {
        fields.push(format!("  year = {{{}}}", date.year));
        if let Some(m) = date.month {
            const MONTHS: [&str; 12] = [
                "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
            ];
            fields.push(format!("  month = {}", MONTHS[(m - 1) as usize]));
        }
    }

    // Every remaining value is escaped too. Volume, pages and identifiers
    // were once written raw as "simple values"; a `}` in any of them closed
    // the field and the entry re-imported as nothing.
    let (number, eid) = number_and_eid(item, bib_type == "article");
    push_escaped(&mut fields, "volume", text_field(item, "volume"));
    push_escaped(&mut fields, "number", number);
    push_escaped(&mut fields, "pages", text_field(item, "page"));
    push_escaped(&mut fields, "eid", eid);

    // Publisher. BibTeX styles read `school` for theses and `institution`
    // for tech reports and ignore `publisher` there — the university was
    // dropped from the formatted reference.
    let publisher_field = match bib_type {
        "phdthesis" | "mastersthesis" => "school",
        "techreport" => "institution",
        _ => "publisher",
    };
    push_escaped(&mut fields, publisher_field, text_field(item, "publisher"));
    push_escaped(&mut fields, "address", text_field(item, "publisher-place"));

    // Identifiers
    push_verbatim(&mut fields, "doi", text_field(item, "DOI"));
    push_verbatim(&mut fields, "url", text_field(item, "URL"));
    // Access date: not a classic BibTeX field, but the one Zotero's BibTeX
    // export writes and JabRef reads; styles that don't know it ignore it.
    push_escaped(&mut fields, "urldate", iso_date(item, "accessed"));
    push_escaped(&mut fields, "isbn", text_field(item, "ISBN"));
    push_escaped(&mut fields, "issn", text_field(item, "ISSN"));

    push_escaped(&mut fields, "abstract", text_field(item, "abstract"));

    // Keywords — CSL `keyword` (singular, string OR v1.0.2 array) → BibTeX
    // `keywords` (plural, comma-separated).
    push_escaped(&mut fields, "keywords", csl_keyword_as_string(item));

    push_escaped(&mut fields, "note", text_field(item, "note"));
    push_escaped(&mut fields, "series", text_field(item, "collection-title"));
    push_escaped(&mut fields, "chapter", text_field(item, "chapter-number"));

    // PMID / PMCID — non-standard BibTeX but widely accepted (JabRef, Zotero)
    push_escaped(&mut fields, "pmid", text_field(item, "PMID"));
    push_escaped(&mut fields, "pmcid", text_field(item, "PMCID"));

    // Eprint (from CSL custom.eprint) — common BibLaTeX-ism but accepted by
    // most BibTeX tooling, and losing it on export would defeat roundtrip.
    push_eprint(&mut fields, item);

    push_escaped(&mut fields, "edition", text_field(item, "edition"));

    format!("@{}{{{},\n{}\n}}", bib_type, key, fields.join(",\n"))
}

/// Push `name = {value}` with the value TeX-escaped.
pub(crate) fn push_escaped(fields: &mut Vec<String>, name: &str, value: Option<String>) {
    if let Some(v) = value {
        fields.push(format!("  {name} = {{{}}}", escape_bibtex(&v)));
    }
}

/// Push `name = {value}` for a verbatim field (see `escape_verbatim`).
pub(crate) fn push_verbatim(fields: &mut Vec<String>, name: &str, value: Option<String>) {
    if let Some(v) = value {
        fields.push(format!("  {name} = {{{}}}", escape_verbatim(&v)));
    }
}

/// Push `eprint`/`eprinttype`/`eprintclass` from CSL `custom.eprint`.
/// Returns whether an eprint was written.
pub(crate) fn push_eprint(fields: &mut Vec<String>, item: &Value) -> bool {
    let eprint = &item["custom"]["eprint"];
    let Some(id) = text_field(eprint, "id") else {
        return false;
    };
    push_verbatim(fields, "eprint", Some(id));
    push_escaped(fields, "eprinttype", text_field(eprint, "type"));
    push_escaped(fields, "eprintclass", text_field(eprint, "class"));
    true
}

/// Convert multiple CSL-JSON items to a BibTeX file string.
///
/// Duplicate cite keys are disambiguated with `a`, `b`, `c`, … suffixes so the
/// output is always parseable — BibTeX rejects repeated keys. Whether the
/// collision came from an explicit user `id` or from `generate_key` fallback,
/// we still dedup: a duplicate key makes the whole file invalid.
pub fn csl_json_array_to_bibtex(items: &[Value]) -> String {
    let keys = disambiguate_keys(items);
    let mut out = items
        .iter()
        .zip(keys.iter())
        .map(|(item, key)| csl_json_to_bibtex_with_key(item, key))
        .collect::<Vec<_>>()
        .join("\n\n");
    out.push('\n');
    out
}

/// Resolve each item's cite key and append `a`/`b`/`c`/… suffixes on
/// collisions. Preserved in insertion order: the first occurrence keeps the
/// bare key, subsequent occurrences get `a`, `b`, `c`, … — skipping any
/// suffixed key already in the file. Counting suffixes blindly turned
/// `["smith2024", "smith2024a", "smith2024"]` into two `smith2024a`. Keys
/// are compared without regard to case, as classic BibTeX compares them.
///
/// Shared with the BibLaTeX exporter so both emit consistent keys when the
/// same array is exported in multiple formats.
pub(crate) fn disambiguate_keys(items: &[Value]) -> Vec<String> {
    unique_keys(
        items.iter().map(resolve_key).collect(),
        |base, n| format!("{base}{}", suffix_for(n)),
        str::to_lowercase,
    )
}

/// Alphabetic suffix for the Nth (1-indexed) duplicate: 1→`a`, 2→`b`, …,
/// 26→`z`, 27→`aa`, 28→`ab`, …
fn suffix_for(mut n: usize) -> String {
    let mut out = String::new();
    loop {
        let digit = ((n - 1) % 26) as u8;
        out.insert(0, (b'a' + digit) as char);
        n = (n - 1) / 26;
        if n == 0 {
            break;
        }
    }
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
        assert!(
            bib.starts_with("@article{smith2024,"),
            "should start with @article: {bib}"
        );
        assert!(
            bib.contains("author = {Smith, John and Doe, Jane}"),
            "should have authors: {bib}"
        );
        assert!(
            bib.contains("title = {A Study of Something}"),
            "should have title: {bib}"
        );
        assert!(
            bib.contains("journal = {Journal of Testing}"),
            "should have journal: {bib}"
        );
        assert!(bib.contains("year = {2024}"), "should have year: {bib}");
        assert!(
            bib.contains("doi = {10.1234/test.2024}"),
            "should have doi: {bib}"
        );
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
        assert!(
            bib.contains("author = {{World Health Organization}}"),
            "literal author should be braced: {bib}"
        );
    }

    #[test]
    fn test_export_month_as_macro() {
        // BibTeX `month` must be a 3-letter macro (no braces), not a number.
        let item = json!({
            "type": "article-journal",
            "id": "x2024",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024, 3, 15]]}
        });
        let bib = csl_json_to_bibtex(&item);
        assert!(bib.contains("month = mar"), "month should be macro: {bib}");
        assert!(
            !bib.contains("month = {3}"),
            "month must not be numeric-braced: {bib}"
        );
        assert!(
            !bib.contains("month = {mar}"),
            "month macro must not be braced: {bib}"
        );
    }

    #[test]
    fn test_export_preserves_eprint() {
        let item = json!({
            "type": "article-journal",
            "id": "arxiv2301",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "custom": {"eprint": {"id": "2301.12345", "type": "arxiv", "class": "cs.CL"}}
        });
        let bib = csl_json_to_bibtex(&item);
        assert!(bib.contains("eprint = {2301.12345}"), "eprint id: {bib}");
        assert!(bib.contains("eprinttype = {arxiv}"), "eprint type: {bib}");
        assert!(bib.contains("eprintclass = {cs.CL}"), "eprint class: {bib}");
    }

    #[test]
    fn test_keyword_accepts_array_form() {
        // CSL v1.0.2 permits `keyword: ["a", "b"]`. Helper collapses to string.
        assert_eq!(
            csl_keyword_as_string(&json!({"keyword": ["ml", "nlp", "  transformers  "]})),
            Some("ml, nlp, transformers".to_string())
        );
        // v1.0.1 string form still works.
        assert_eq!(
            csl_keyword_as_string(&json!({"keyword": "ml, nlp"})),
            Some("ml, nlp".to_string())
        );
        // Array of mixed garbage → filtered out cleanly.
        assert_eq!(
            csl_keyword_as_string(&json!({"keyword": ["", "  ", "real", null, 42]})),
            Some("real".to_string())
        );
        // Empty / missing → None, no spurious `keywords = {}`.
        assert_eq!(csl_keyword_as_string(&json!({})), None);
        assert_eq!(csl_keyword_as_string(&json!({"keyword": ""})), None);
        assert_eq!(csl_keyword_as_string(&json!({"keyword": []})), None);
    }

    #[test]
    fn test_export_keyword_array_form_reaches_bibtex() {
        // Regression guard: array-form keyword must not be silently dropped.
        let item = json!({
            "type": "article-journal",
            "id": "x2024",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "keyword": ["ml", "nlp"]
        });
        let bib = csl_json_to_bibtex(&item);
        assert!(
            bib.contains("keywords = {ml, nlp}"),
            "array form must export: {bib}"
        );
    }

    #[test]
    fn test_export_keywords_singular_csl_to_plural_bibtex() {
        let item = json!({
            "type": "article-journal",
            "id": "x2024",
            "title": "T",
            "author": [{"family": "X"}],
            "issued": {"date-parts": [[2024]]},
            "keyword": "ml, nlp"
        });
        let bib = csl_json_to_bibtex(&item);
        assert!(
            bib.contains("keywords = {ml, nlp}"),
            "CSL `keyword` (singular) → BibTeX `keywords` (plural): {bib}"
        );
    }

    #[test]
    fn test_export_preserves_note_series_chapter_pmid() {
        let item = json!({
            "type": "chapter",
            "id": "k2023",
            "title": "Chapter T",
            "author": [{"family": "K"}],
            "issued": {"date-parts": [[2023]]},
            "note": "Funded by NSF",
            "collection-title": "Studies in X",
            "chapter-number": "7",
            "PMID": "12345678",
            "PMCID": "PMC9876543"
        });
        let bib = csl_json_to_bibtex(&item);
        assert!(bib.contains("note = {Funded by NSF}"), "note: {bib}");
        assert!(bib.contains("series = {Studies in X}"), "series: {bib}");
        assert!(bib.contains("chapter = {7}"), "chapter: {bib}");
        assert!(bib.contains("pmid = {12345678}"), "pmid: {bib}");
        assert!(bib.contains("pmcid = {PMC9876543}"), "pmcid: {bib}");
    }

    #[test]
    fn test_roundtrip_bibtex_keeps_abstract() {
        let input = r#"@article{smith2024, author = {Smith, J.}, title = {T}, journal = {J}, year = {2024}, abstract = {This is the abstract.}, keywords = {ml, nlp}, note = {funded}}"#;
        let parsed =
            crate::parsers::bibtex::parse_bibtex(input, &crate::parsers::ParseOptions::default());
        assert_eq!(parsed.entries.len(), 1);
        let exported = csl_json_to_bibtex(&parsed.entries[0]);
        assert!(
            exported.contains("abstract = {This is the abstract.}"),
            "abstract survives: {exported}"
        );
        assert!(
            exported.contains("keywords = {"),
            "keywords survive: {exported}"
        );
        assert!(
            exported.contains("note = {funded}"),
            "note survives: {exported}"
        );
    }

    #[test]
    fn test_array_dedup_keys() {
        // Three Smith-2024 entries — without dedup, BibTeX rejects the file.
        let items = vec![
            json!({"type": "article-journal", "author": [{"family": "Smith"}],
                   "title": "A", "issued": {"date-parts": [[2024]]}}),
            json!({"type": "article-journal", "author": [{"family": "Smith"}],
                   "title": "B", "issued": {"date-parts": [[2024]]}}),
            json!({"type": "article-journal", "author": [{"family": "Smith"}],
                   "title": "C", "issued": {"date-parts": [[2024]]}}),
        ];
        let out = csl_json_array_to_bibtex(&items);
        assert!(
            out.contains("@article{Smith2024,"),
            "first entry bare key: {out}"
        );
        assert!(
            out.contains("@article{Smith2024a,"),
            "second gets 'a': {out}"
        );
        assert!(
            out.contains("@article{Smith2024b,"),
            "third gets 'b': {out}"
        );
    }

    #[test]
    fn test_array_dedup_respects_explicit_id() {
        // Explicit user `id` still gets dedup — the file must be valid BibTeX
        // regardless of where the collision came from.
        let items = vec![
            json!({"type": "article-journal", "id": "custom",
                   "author": [{"family": "X"}], "title": "A",
                   "issued": {"date-parts": [[2024]]}}),
            json!({"type": "article-journal", "id": "custom",
                   "author": [{"family": "Y"}], "title": "B",
                   "issued": {"date-parts": [[2024]]}}),
        ];
        let out = csl_json_array_to_bibtex(&items);
        assert!(
            out.contains("@article{custom,"),
            "first keeps bare id: {out}"
        );
        assert!(
            out.contains("@article{customa,"),
            "second gets suffix: {out}"
        );
    }

    #[test]
    fn test_disambiguate_keys_first_seen_semantics() {
        // Pin the contract: first-seen keeps the bare key; subsequent
        // collisions take `a`, `b`, `c`, ... This is non-commutative — the
        // *set* of keys is stable under reordering, but the binding of
        // cite-key → item is not. Downstream consumers that export the same
        // items in different orders will get the same keys attached to
        // different items. Documented here so a future refactor (e.g. sort
        // before dedup for determinism) doesn't change it silently.
        let a = vec![
            json!({"type": "article-journal", "id": "dup",
                   "author": [{"family": "A"}], "title": "Early",
                   "issued": {"date-parts": [[2024]]}}),
            json!({"type": "article-journal", "id": "dup",
                   "author": [{"family": "B"}], "title": "Late",
                   "issued": {"date-parts": [[2024]]}}),
        ];
        let keys_a = disambiguate_keys(&a);
        assert_eq!(keys_a, vec!["dup", "dupa"], "forward order: {keys_a:?}");

        let b = vec![a[1].clone(), a[0].clone()];
        let keys_b = disambiguate_keys(&b);
        assert_eq!(
            keys_b,
            vec!["dup", "dupa"],
            "reversed order yields same SET: {keys_b:?}"
        );
        // But the item-to-key binding is reversed:
        // in `a`, "Early" gets bare `dup`; in `b`, "Late" gets bare `dup`.
    }

    #[test]
    fn test_name_part_braces_only_what_bibtex_reads_as_structure() {
        assert_eq!(name_part("Procter and Gamble"), "{Procter and Gamble}");
        assert_eq!(name_part("Mary AND John"), "{Mary AND John}");
        assert_eq!(name_part("Smith, Jones"), "{Smith, Jones}");
        assert_eq!(name_part("Smith {x}, Jones"), r"{Smith \{x\}, Jones}");
        assert_eq!(name_part("Anderson"), "Anderson");
        assert_eq!(name_part("Sandandand"), "Sandandand");
        assert_eq!(name_part("da"), "da");
    }

    #[test]
    fn test_number_and_eid_follow_the_entry_type() {
        let both = json!({"issue": "3", "number": "e12"});
        assert_eq!(
            number_and_eid(&both, true),
            (Some("3".into()), Some("e12".into()))
        );
        assert_eq!(number_and_eid(&both, false), (Some("e12".into()), None));
        let issue_only = json!({"issue": 4});
        assert_eq!(number_and_eid(&issue_only, false), (Some("4".into()), None));
    }

    #[test]
    fn test_disambiguate_keys_never_reuses_a_key_in_the_file() {
        let item = |id: &str| json!({"type": "book", "id": id, "title": "T"});
        let keys = disambiguate_keys(&[item("k"), item("ka"), item("k"), item("K")]);
        assert_eq!(keys, vec!["k", "ka", "kb", "Kc"]);
    }

    #[test]
    fn test_suffix_for_rolls_past_z() {
        // 26 collisions exhaust single-letter suffixes; 27+ roll to `aa`, `ab`, …
        assert_eq!(suffix_for(1), "a");
        assert_eq!(suffix_for(26), "z");
        assert_eq!(suffix_for(27), "aa");
        assert_eq!(suffix_for(28), "ab");
        assert_eq!(suffix_for(52), "az");
        assert_eq!(suffix_for(53), "ba");
    }

    #[test]
    fn test_roundtrip_bibtex() {
        // Parse real BibTeX, export back to BibTeX, verify key fields survive
        let input = "@article{test2024, author = {Smith, John}, title = {Test Title}, journal = {Nature}, year = {2024}, volume = {1}, pages = {10-20}, doi = {10.1234/test}}";
        let parsed =
            crate::parsers::bibtex::parse_bibtex(input, &crate::parsers::ParseOptions::default());
        assert_eq!(parsed.entries.len(), 1);

        let exported = csl_json_to_bibtex(&parsed.entries[0]);
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
