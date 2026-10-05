use super::{ParseErrorInfo, ParseOptions, ParseResult};
use serde_json::{json, Value};

/// RIS type tag → CSL-JSON type mapping
fn ris_type_to_csl(ty: &str) -> &'static str {
    match ty {
        "JOUR" | "JFULL" => "article-journal",
        "NEWS" => "article-newspaper",
        "BOOK" | "WHOLE" => "book",
        "CHAP" | "CHAPT" => "chapter",
        "THES" => "thesis",
        "CONF" | "CPAPER" => "paper-conference",
        "RPRT" | "REPORT" => "report",
        "ELEC" | "ICOMM" => "webpage",
        "DATA" | "DBASE" => "dataset",
        "COMP" => "software",
        _ => "article-journal",
    }
}

/// Parse RIS content into CSL-JSON items.
pub fn parse_ris(input: &str, options: &ParseOptions) -> ParseResult {
    if input.trim().is_empty() {
        return ParseResult {
            entries: vec![],
            errors: vec![],
            format: "ris".to_string(),
            truncated: false,
            scanned_entries: 0,
        };
    }

    // DoS guard: reject oversized input before parsing
    if input.len() > options.max_input_bytes {
        return ParseResult {
            entries: vec![],
            errors: vec![ParseErrorInfo {
                preview: format!(
                    "Input size {} bytes exceeds limit {} bytes",
                    input.len(),
                    options.max_input_bytes
                ),
                error: "input too large".to_string(),
            }],
            format: "ris".to_string(),
            truncated: true,
            scanned_entries: 0,
        };
    }

    let mut entries: Vec<Value> = Vec::new();
    let mut current: Option<RisRecord> = None;
    let mut scanned = 0;

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // RIS lines: "XX  - value" — tag, two spaces, a dash, then the value.
        // The space after the dash is conventional, not guaranteed: real
        // exporters emit "TY  -JOUR" too, and requiring it made the parser
        // skip every line of a file `detect_format` had accepted as RIS,
        // returning zero entries and zero errors.
        let (tag, value) = match line.find("  -") {
            Some(pos) => {
                let tag = line[..pos].trim();
                let is_tag =
                    (2..=4).contains(&tag.len()) && tag.chars().all(|c| c.is_ascii_alphanumeric());
                if !is_tag {
                    continue;
                }
                let rest = &line[pos + 3..];
                (tag, rest.strip_prefix(' ').unwrap_or(rest).trim())
            }
            None => continue,
        };

        match tag {
            "TY" => {
                // A TY inside an open record means its ER was missing.
                // Replacing the record silently dropped it.
                if let Some(record) = current.take() {
                    if !push_record(&mut entries, record, &mut scanned, options.max_entries) {
                        return truncated_result(entries, scanned);
                    }
                }
                current = Some(RisRecord::new(value));
            }
            "ER" => {
                if let Some(record) = current.take() {
                    if !push_record(&mut entries, record, &mut scanned, options.max_entries) {
                        return truncated_result(entries, scanned);
                    }
                }
            }
            _ => {
                if let Some(ref mut record) = current {
                    record.add_field(tag, value);
                }
            }
        }
    }

    // Handle entry without trailing ER
    let mut truncated = false;
    if let Some(record) = current.take() {
        truncated = !push_record(&mut entries, record, &mut scanned, options.max_entries);
    }

    ParseResult {
        entries,
        errors: vec![],
        format: "ris".to_string(),
        truncated,
        scanned_entries: scanned,
    }
}

/// Count a finished record and keep it unless `max_entries` is reached.
/// Returns `false` when the record was dropped for the limit.
fn push_record(
    entries: &mut Vec<Value>,
    record: RisRecord,
    scanned: &mut usize,
    max_entries: Option<usize>,
) -> bool {
    *scanned += 1;
    if max_entries.is_some_and(|max| entries.len() >= max) {
        return false;
    }
    entries.push(record.finish());
    true
}

fn truncated_result(entries: Vec<Value>, scanned: usize) -> ParseResult {
    ParseResult {
        entries,
        errors: vec![],
        format: "ris".to_string(),
        truncated: true,
        scanned_entries: scanned,
    }
}

/// `YYYY/MM/DD/other` (the RIS form) or ISO `YYYY-MM-DD` → CSL date. Month
/// and day are kept only when in range; out-of-range parts fall back to the
/// coarser precision instead of producing an impossible date.
fn parse_ris_date(value: &str) -> Option<Value> {
    let mut parts = value.split(['/', '-']).map(str::trim);
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts
        .next()
        .and_then(|m| m.parse::<i32>().ok())
        .filter(|m| (1..=12).contains(m));
    let day = parts
        .next()
        .and_then(|d| d.parse::<i32>().ok())
        .filter(|d| (1..=31).contains(d));
    Some(match (month, day) {
        (Some(m), Some(d)) => json!({"date-parts": [[year, m, d]]}),
        (Some(m), None) => json!({"date-parts": [[year, m]]}),
        _ => json!({"date-parts": [[year]]}),
    })
}

/// RIS name: "Last, First" or "Last, First, Suffix"; anything without a
/// comma is kept whole as a literal.
fn parse_ris_name(value: &str) -> Value {
    let mut parts = value.splitn(3, ',').map(str::trim);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(family), Some(given), suffix) => {
            let mut name = json!({"family": family, "given": given});
            if let Some(suffix) = suffix.filter(|s| !s.is_empty()) {
                name["suffix"] = json!(suffix);
            }
            name
        }
        _ => json!({"literal": value}),
    }
}

/// One record being read, finished into CSL-JSON at `ER` (or wherever the
/// record ends) — the only place fields that need the whole record are
/// resolved: pages from SP/EP, container title from full vs. abbreviated
/// journal tags.
struct RisRecord {
    csl_type: &'static str,
    fields: serde_json::Map<String, Value>,
    authors: Vec<Value>,
    editors: Vec<Value>,
    host_editors: Vec<Value>,
    keywords: Vec<String>,
    start_page: Option<String>,
    end_page: Option<String>,
    journal_full: Option<String>,
    journal_abbr: Option<String>,
}

impl RisRecord {
    fn new(ty: &str) -> Self {
        Self {
            csl_type: ris_type_to_csl(ty),
            fields: serde_json::Map::new(),
            authors: Vec::new(),
            editors: Vec::new(),
            host_editors: Vec::new(),
            keywords: Vec::new(),
            start_page: None,
            end_page: None,
            journal_full: None,
            journal_abbr: None,
        }
    }

    fn add_field(&mut self, tag: &str, value: &str) {
        let entry = &mut self.fields;
        match tag {
            "AU" | "A1" => self.authors.push(parse_ris_name(value)),
            // ED is always an editor; A2 is the book's editor for chapters
            // and proceedings papers (for other types it means series
            // editor, performer, … and stays unmapped).
            "ED" => self.editors.push(parse_ris_name(value)),
            "A2" if matches!(self.csl_type, "chapter" | "paper-conference") => {
                self.host_editors.push(parse_ris_name(value))
            }
            "TI" | "T1" => {
                entry.insert("title".into(), json!(value));
            }
            // BT is the primary title of a book in the RIS spec (TI still
            // wins when both are present); for parts it names the host book.
            "BT" if self.csl_type == "book" => {
                entry.entry("title").or_insert_with(|| json!(value));
            }
            // Full container titles beat abbreviations whatever their order.
            "T2" | "JF" | "BT" => self.journal_full = Some(value.to_string()),
            "JO" | "JA" | "J2" => self.journal_abbr = Some(value.to_string()),
            "PY" | "Y1" => {
                // DA carries the date proper; PY only fills in when DA
                // hasn't set one.
                if !entry.contains_key("issued") {
                    if let Some(date) = parse_ris_date(value) {
                        entry.insert("issued".into(), date);
                    }
                }
            }
            "DA" => {
                if let Some(date) = parse_ris_date(value) {
                    entry.insert("issued".into(), date);
                }
            }
            // Y2 is the access date in the RIS spec and in Zotero/EndNote
            // exports, for every type (conference papers included). It is
            // never the publication date — mapping it to `issued` let it
            // overwrite DA.
            "Y2" => {
                if let Some(date) = parse_ris_date(value) {
                    entry.insert("accessed".into(), date);
                }
            }
            "VL" => {
                entry.insert("volume".into(), json!(value));
            }
            "IS" => {
                entry.insert("issue".into(), json!(value));
            }
            "SP" => self.start_page = Some(value.to_string()),
            "EP" => self.end_page = Some(value.to_string()),
            "DO" => {
                entry.insert("DOI".into(), json!(value));
            }
            "UR" => {
                entry.insert("URL".into(), json!(value));
            }
            "PB" => {
                entry.insert("publisher".into(), json!(value));
            }
            "CY" => {
                entry.insert("publisher-place".into(), json!(value));
            }
            "SN" => {
                if value.contains('-') && value.len() == 9 {
                    entry.insert("ISSN".into(), json!(value));
                } else {
                    entry.insert("ISBN".into(), json!(value));
                }
            }
            "AB" | "N2" => {
                entry.insert("abstract".into(), json!(value));
            }
            "LA" => {
                entry.insert("language".into(), json!(value));
            }
            "KW" => self.keywords.push(value.to_string()),
            _ => {}
        }
    }

    fn finish(self) -> Value {
        let mut entry = self.fields;
        entry.insert("type".into(), json!(self.csl_type));
        if !self.authors.is_empty() {
            entry.insert("author".into(), json!(self.authors));
        }
        // A2 and ED may both name the host book's editors; list a person
        // once. Repeats within one tag stay — two "Wang, L" can be two people.
        let mut editors = self.host_editors.clone();
        editors.extend(
            self.editors
                .into_iter()
                .filter(|e| !self.host_editors.contains(e)),
        );
        if !editors.is_empty() {
            entry.insert("editor".into(), json!(editors));
        }
        if !self.keywords.is_empty() {
            entry.insert("keyword".into(), json!(self.keywords.join(", ")));
        }

        // SP alone is common — article numbers ("e1234"), or exporters that
        // put the whole range in SP — and must survive without an EP.
        let page = match (self.start_page, self.end_page) {
            (Some(sp), Some(ep)) if sp != ep => Some(format!("{sp}-{ep}")),
            (Some(sp), _) => Some(sp),
            (None, ep) => ep,
        };
        if let Some(page) = page {
            entry.insert("page".into(), json!(page));
        }

        match (self.journal_full, self.journal_abbr) {
            (Some(full), abbr) => {
                entry.insert("container-title".into(), json!(full));
                if let Some(abbr) = abbr {
                    entry.insert("container-title-short".into(), json!(abbr));
                }
            }
            (None, Some(abbr)) => {
                entry.insert("container-title".into(), json!(abbr));
            }
            (None, None) => {}
        }

        Value::Object(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn test_parse_ris_comp_type() {
        let input = "TY  - COMP\nTI  - My Software\nPY  - 2024\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result.entries[0]["type"], "software",
            "COMP should map to software"
        );
    }

    #[test]
    fn test_parse_ris_news_type() {
        let input = "TY  - NEWS\nTI  - Breaking Story\nPY  - 2024\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result.entries[0]["type"], "article-newspaper",
            "NEWS should map to article-newspaper"
        );
    }

    #[test]
    fn test_parse_ris_da_full_date() {
        let input = "TY  - JOUR\nTI  - Date Test\nPY  - 2024///\nDA  - 2024/01/14/\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let issued = &result.entries[0]["issued"]["date-parts"][0];
        assert_eq!(issued[0], 2024, "year from DA");
        assert_eq!(issued[1], 1, "month from DA");
        assert_eq!(issued[2], 14, "day from DA");
    }

    #[test]
    fn test_parse_ris_da_month_only() {
        let input = "TY  - JOUR\nTI  - Month Test\nDA  - 2024/03/\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let issued = &result.entries[0]["issued"]["date-parts"][0];
        assert_eq!(issued[0], 2024);
        assert_eq!(issued[1], 3);
        assert!(
            issued.get(2).is_none() || issued[2].is_null(),
            "no day when only month given"
        );
    }

    #[test]
    fn test_parse_ris_da_overrides_py() {
        let input = "TY  - JOUR\nPY  - 2024///\nDA  - 2024/06/15/\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let issued = &result.entries[0]["issued"]["date-parts"][0];
        assert_eq!(issued[1], 6, "DA should override PY's year-only date");
    }

    #[test]
    fn test_parse_ris_keywords_without_er() {
        let input = "TY  - JOUR\nTI  - No Terminator\nKW  - machine learning\nKW  - AI";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result.entries[0]["keyword"], "machine learning, AI",
            "keywords should be preserved even without ER terminator"
        );
    }

    #[test]
    fn test_parse_ris_y2_is_access_date_not_publication_date() {
        // Zotero and EndNote write the access date in Y2. Mapping it to
        // `issued` let it overwrite DA/PY: a 2019 article imported as 2024.
        let input = "TY  - JOUR\nTI  - T\nPY  - 2019\nDA  - 2019/05/01/\nY2  - 2024/01/15/\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let entry = &result.entries[0];
        assert_eq!(entry["issued"]["date-parts"][0], json!([2019, 5, 1]));
        assert_eq!(entry["accessed"]["date-parts"][0], json!([2024, 1, 15]));
    }

    #[test]
    fn test_parse_ris_y2_on_conference_is_still_access_date() {
        // Zotero and EndNote write the access date to Y2 for every type;
        // reading it as the event date printed "Paper presented <access
        // date>" in Chicago.
        let input = "TY  - CONF\nTI  - Talk\nDA  - 2019/06//\nY2  - 2024/01/15/12:34:56\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let entry = &result.entries[0];
        assert_eq!(entry["issued"]["date-parts"][0], json!([2019, 6]));
        assert_eq!(entry["accessed"]["date-parts"][0], json!([2024, 1, 15]));
        assert!(entry.get("event-date").is_none(), "{entry}");
    }

    #[test]
    fn test_parse_ris_bt_is_the_title_of_a_book() {
        // BT is the primary title for BOOK; as a container it rendered
        // "The Book. In The Book."
        let input = "TY  - BOOK\nTI  - The Book\nBT  - The Book\nER  - \nTY  - BOOK\nBT  - Only BT\nER  - \nTY  - CHAP\nTI  - Ch\nBT  - Host Book\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["title"], "The Book");
        assert!(
            result.entries[0].get("container-title").is_none(),
            "{}",
            result.entries[0]
        );
        assert_eq!(result.entries[1]["title"], "Only BT");
        assert_eq!(result.entries[2]["container-title"], "Host Book");
    }

    #[test]
    fn test_parse_ris_same_editor_in_ed_and_a2_is_listed_once() {
        let input = "TY  - CHAP\nTI  - Ch\nA2  - Doe, Jane\nED  - Doe, Jane\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["editor"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_ris_start_page_alone_is_kept() {
        // Article numbers ("e1234") and exporters that put the whole range
        // in SP have no EP; the page used to be dropped with the scratch key.
        let input = "TY  - JOUR\nTI  - A\nSP  - e1234\nER  - \nTY  - JOUR\nTI  - B\nSP  - 100-115\nER  - \nTY  - JOUR\nTI  - C\nSP  - 7\nEP  - 9\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["page"], "e1234");
        assert_eq!(result.entries[1]["page"], "100-115");
        assert_eq!(result.entries[2]["page"], "7-9");
    }

    #[test]
    fn test_parse_ris_truncated_output_has_no_scratch_keys() {
        let input = "TY  - JOUR\nTI  - A\nSP  - 5\nER  - \nTY  - JOUR\nTI  - B\nER  - ";
        let opts = ParseOptions {
            max_entries: Some(1),
            ..Default::default()
        };
        let result = parse_ris(input, &opts);
        assert!(result.truncated);
        let entry = result.entries[0].as_object().unwrap();
        assert!(
            entry.keys().all(|k| !k.starts_with('_')),
            "internal keys leaked: {entry:?}"
        );
        assert_eq!(entry["page"], "5");
    }

    #[test]
    fn test_parse_ris_full_journal_title_beats_abbreviation() {
        // T2/JF carry the full title, JO/JA/J2 the abbreviation. Last-wins
        // replaced "Nature Medicine" with "Nat Med" whenever JO came later.
        for input in [
            "TY  - JOUR\nTI  - A\nT2  - Nature Medicine\nJO  - Nat Med\nER  - ",
            "TY  - JOUR\nTI  - A\nJO  - Nat Med\nT2  - Nature Medicine\nER  - ",
            "TY  - JOUR\nTI  - A\nJF  - Nature Medicine\nJ2  - Nat Med\nER  - ",
        ] {
            let result = parse_ris(input, &ParseOptions::default());
            let entry = &result.entries[0];
            assert_eq!(entry["container-title"], "Nature Medicine", "{input}");
            assert_eq!(entry["container-title-short"], "Nat Med", "{input}");
        }
        // An abbreviation alone is still the best container title there is.
        let result = parse_ris(
            "TY  - JOUR\nTI  - A\nJO  - Nature\nER  - ",
            &ParseOptions::default(),
        );
        assert_eq!(result.entries[0]["container-title"], "Nature");
    }

    #[test]
    fn test_parse_ris_missing_er_does_not_swallow_the_record() {
        let input = "TY  - JOUR\nTI  - First\nAU  - Smith, J\nTY  - BOOK\nTI  - Second\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 2, "{:?}", result.entries);
        assert_eq!(result.entries[0]["title"], "First");
        assert_eq!(result.entries[0]["author"][0]["family"], "Smith");
        assert_eq!(result.entries[1]["title"], "Second");
        assert!(result.entries[1].get("author").is_none());
    }

    #[test]
    fn test_parse_ris_dates_accept_iso_and_full_py() {
        let issued = |input: &str| {
            parse_ris(input, &ParseOptions::default()).entries[0]["issued"]["date-parts"][0].clone()
        };
        assert_eq!(
            issued("TY  - JOUR\nDA  - 2024-05-12\nER  - "),
            json!([2024, 5, 12])
        );
        assert_eq!(
            issued("TY  - JOUR\nPY  - 2024/05/12/\nER  - "),
            json!([2024, 5, 12])
        );
        assert_eq!(issued("TY  - JOUR\nPY  - 2024///\nER  - "), json!([2024]));
        assert_eq!(
            issued("TY  - JOUR\nDA  - 2024/13/40/\nER  - "),
            json!([2024])
        );
    }

    #[test]
    fn test_parse_ris_editors_and_name_suffix() {
        let input = "TY  - CHAP\nAU  - King, Martin Luther, Jr.\nA2  - Doe, Jane\nED  - Roe, Rich\nTI  - Ch\nT2  - The Book\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let entry = &result.entries[0];
        assert_eq!(
            entry["author"][0],
            json!({"family": "King", "given": "Martin Luther", "suffix": "Jr."})
        );
        let editors = entry["editor"].as_array().expect("editors");
        assert_eq!(editors.len(), 2, "{editors:?}");
        assert_eq!(editors[0]["family"], "Doe");
        assert_eq!(editors[1]["family"], "Roe");
        assert_eq!(entry["container-title"], "The Book");
    }

    #[test]
    fn test_roundtrip_ris_software() {
        let item = serde_json::json!({
            "type": "software",
            "title": "My App",
            "issued": {"date-parts": [[2024, 3, 15]]}
        });
        let exported = crate::export::ris::csl_json_to_ris(&item);
        assert!(
            exported.contains("TY  - COMP"),
            "software should export as COMP"
        );

        let reimported = parse_ris(&exported, &ParseOptions::default());
        assert_eq!(
            reimported.entries[0]["type"], "software",
            "COMP should roundtrip to software"
        );
        let issued = &reimported.entries[0]["issued"]["date-parts"][0];
        assert_eq!(issued[0], 2024);
        assert_eq!(issued[1], 3);
        assert_eq!(issued[2], 15, "full date should survive roundtrip");
    }
}
