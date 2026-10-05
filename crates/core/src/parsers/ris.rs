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
        "PAT" => "patent",
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
        // A byte-order mark (Windows exporters write one) is not part of the
        // first tag: left in place it turned "TY" into "\u{FEFF}TY" and the
        // whole first record was dropped.
        let line = line.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
        if line.is_empty() {
            continue;
        }

        let Some((tag, value)) = split_tag_line(line) else {
            // A line without a tag continues the field above it: exporters
            // wrap long titles and abstracts.
            if let Some(ref mut record) = current {
                record.continue_field(line);
            }
            continue;
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

/// RIS lines: "XX  - value" — an uppercase tag, two spaces, a dash, then
/// the value. The space after the dash is conventional, not guaranteed: real
/// exporters emit "TY  -JOUR" too, and requiring it made the parser skip
/// every line of a file `detect_format` had accepted as RIS, returning zero
/// entries and zero errors. Tags are uppercase, so "and  - " inside wrapped
/// prose stays text.
fn split_tag_line(line: &str) -> Option<(&str, &str)> {
    let pos = line.find("  -")?;
    let tag = line[..pos].trim();
    let is_tag = (2..=4).contains(&tag.len())
        && tag.starts_with(|c: char| c.is_ascii_uppercase())
        && tag
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !is_tag {
        return None;
    }
    let rest = &line[pos + 3..];
    Some((tag, rest.strip_prefix(' ').unwrap_or(rest).trim()))
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

/// A RIS date read into a CSL date.
struct RisDate {
    date: Value,
    /// Written year first, so its month and day mean what they say.
    year_first: bool,
}

/// `YYYY/MM/DD/other` (the RIS form) or ISO `YYYY-MM-DD` → CSL date. Month
/// and day are kept only when in range; out-of-range parts fall back to the
/// coarser precision instead of producing an impossible date.
///
/// Only a four-digit year is a year. "05/12/2019" (EndNote with US or
/// European settings) is day/month or month/day: its first number used to
/// become year 5. All that is certain there is the trailing year.
fn parse_ris_date(value: &str) -> Option<RisDate> {
    fn four_digit_year(part: &str) -> Option<i32> {
        if part.len() == 4 && part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    }
    let parts: Vec<&str> = value.split(['/', '-']).map(str::trim).collect();
    let Some(year) = four_digit_year(parts[0]) else {
        let year = parts
            .iter()
            .rev()
            .find(|p| !p.is_empty())
            .and_then(|p| four_digit_year(p))?;
        return Some(RisDate {
            date: json!({"date-parts": [[year]]}),
            year_first: false,
        });
    };
    let month = parts
        .get(1)
        .and_then(|m| m.parse::<i32>().ok())
        .filter(|m| (1..=12).contains(m));
    let day = parts
        .get(2)
        .and_then(|d| d.parse::<i32>().ok())
        .filter(|d| (1..=31).contains(d));
    let date = match (month, day) {
        (Some(m), Some(d)) => json!({"date-parts": [[year, m, d]]}),
        (Some(m), None) => json!({"date-parts": [[year, m]]}),
        _ => json!({"date-parts": [[year]]}),
    };
    Some(RisDate {
        date,
        year_first: true,
    })
}

/// `SN` holds an ISSN for serials and an ISBN for books. PubMed-derived
/// files qualify ISSNs ("1476-4687 (Electronic)") and some exporters drop
/// the hyphen ("00280836"); both forms were filed as ISBNs.
fn as_issn(value: &str) -> Option<&str> {
    let number = value.split('(').next()?.trim();
    let bytes = number.as_bytes();
    let digits: Vec<u8> = match bytes.len() {
        8 => bytes.to_vec(),
        9 if bytes[4] == b'-' => [&bytes[..4], &bytes[5..]].concat(),
        _ => return None,
    };
    let is_issn = digits[..7].iter().all(u8::is_ascii_digit)
        && matches!(digits[7], b'0'..=b'9' | b'X' | b'x');
    is_issn.then_some(number)
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
    /// From DA: the date proper, when written year first.
    date: Option<RisDate>,
    /// From PY/Y1; the first one wins.
    year: Option<RisDate>,
    /// Where the last tag's text went, for a wrapped line to follow it.
    last: Slot,
}

/// What a line without a tag adds to, after the last tag.
#[derive(Clone, Copy)]
enum Slot {
    /// A prose field: the line is its wrapped text.
    Text(&'static str),
    JournalFull,
    JournalAbbr,
    /// One keyword per line: the line is another keyword.
    Keyword,
    None,
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
            date: None,
            year: None,
            last: Slot::None,
        }
    }

    /// Set a prose field; a wrapped line after it continues it.
    fn set_text(&mut self, key: &'static str, value: &str) -> Slot {
        self.fields.insert(key.into(), json!(value));
        Slot::Text(key)
    }

    /// Set a one-value field (an identifier, a number). EndNote writes one
    /// value per line, so a line after it is another value, not more of
    /// this one — appending it turned two URLs into one broken URL.
    fn set_value(&mut self, key: &'static str, value: &str) -> Slot {
        self.fields.insert(key.into(), json!(value));
        Slot::None
    }

    fn add_field(&mut self, tag: &str, value: &str) {
        self.last = match tag {
            "AU" | "A1" => {
                self.authors.push(parse_ris_name(value));
                Slot::None
            }
            // ED is always an editor; A2 is the book's editor for chapters
            // and proceedings papers (for other types it means series
            // editor, performer, … and stays unmapped).
            "ED" => {
                self.editors.push(parse_ris_name(value));
                Slot::None
            }
            "A2" if matches!(self.csl_type, "chapter" | "paper-conference") => {
                self.host_editors.push(parse_ris_name(value));
                Slot::None
            }
            "TI" | "T1" => self.set_text("title", value),
            // BT is the primary title of a book in the RIS spec (TI still
            // wins when both are present); for parts it names the host book.
            "BT" if self.csl_type == "book" => {
                if self.fields.contains_key("title") {
                    Slot::None
                } else {
                    self.set_text("title", value)
                }
            }
            // Full container titles beat abbreviations whatever their order.
            "T2" | "JF" | "BT" => {
                self.journal_full = Some(value.to_string());
                Slot::JournalFull
            }
            "JO" | "JA" | "J2" => {
                self.journal_abbr = Some(value.to_string());
                Slot::JournalAbbr
            }
            "PY" | "Y1" => {
                if self.year.is_none() {
                    self.year = parse_ris_date(value);
                }
                Slot::None
            }
            "DA" => {
                if let Some(date) = parse_ris_date(value) {
                    self.date = Some(date);
                }
                Slot::None
            }
            // Y2 is the access date in the RIS spec and in Zotero/EndNote
            // exports, for every type (conference papers included). It is
            // never the publication date — mapping it to `issued` let it
            // overwrite DA.
            "Y2" => {
                if let Some(date) = parse_ris_date(value) {
                    self.fields.insert("accessed".into(), date.date);
                }
                Slot::None
            }
            "VL" => self.set_value("volume", value),
            "IS" => self.set_value("issue", value),
            "ET" => self.set_value("edition", value),
            // The item's own number: C7 for an article, M1 for other types,
            // as the exporter writes them.
            "C7" | "M1" if !self.fields.contains_key("number") => self.set_value("number", value),
            "SP" => {
                self.start_page = Some(value.to_string());
                Slot::None
            }
            "EP" => {
                self.end_page = Some(value.to_string());
                Slot::None
            }
            "DO" => self.set_value("DOI", value),
            "UR" => self.set_value("URL", value),
            "PB" => self.set_text("publisher", value),
            "CY" => self.set_text("publisher-place", value),
            "SN" => {
                match as_issn(value) {
                    Some(issn) => self.fields.insert("ISSN".into(), json!(issn)),
                    None => self.fields.insert("ISBN".into(), json!(value)),
                };
                Slot::None
            }
            "AB" | "N2" => self.set_text("abstract", value),
            "LA" => self.set_value("language", value),
            "KW" => {
                self.keywords.push(value.to_string());
                Slot::Keyword
            }
            _ => Slot::None,
        };
    }

    /// A line without a tag: wrapped text for the prose field above it, or
    /// one more keyword. After a one-value field it is dropped.
    fn continue_field(&mut self, more: &str) {
        let text = match self.last {
            Slot::Text(key) => match self.fields.get_mut(key) {
                Some(Value::String(text)) => text,
                _ => return,
            },
            Slot::JournalFull => match self.journal_full.as_mut() {
                Some(text) => text,
                None => return,
            },
            Slot::JournalAbbr => match self.journal_abbr.as_mut() {
                Some(text) => text,
                None => return,
            },
            Slot::Keyword => {
                self.keywords.push(more.to_string());
                return;
            }
            Slot::None => return,
        };
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(more);
    }

    fn finish(self) -> Value {
        let mut entry = self.fields;
        entry.insert("type".into(), json!(self.csl_type));
        if !self.authors.is_empty() {
            entry.insert("author".into(), json!(self.authors));
        }
        // DA carries the date proper; PY fills in when DA is missing or
        // wasn't written year first.
        let issued = match (self.date, self.year) {
            (Some(da), _) if da.year_first => Some(da),
            (None, py) => py,
            (Some(_), Some(py)) if py.year_first => Some(py),
            (da, _) => da,
        };
        if let Some(issued) = issued {
            entry.insert("issued".into(), issued.date);
        }
        // A2 and ED may both name the host book's editors; list a person
        // once. Repeats within one tag stay — two "Wang, L" can be two people.
        let host: std::collections::HashSet<String> =
            self.host_editors.iter().map(Value::to_string).collect();
        let mut editors = self.host_editors;
        editors.extend(
            self.editors
                .into_iter()
                .filter(|e| !host.contains(&e.to_string())),
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
    fn test_parse_ris_utf8_bom_keeps_the_first_record() {
        // Windows exporters prefix a byte-order mark. It glued itself to the
        // first tag ("\u{FEFF}TY"), so the first record's fields had no
        // record to land in and vanished without an error.
        let input = "\u{FEFF}TY  - JOUR\nTI  - First\nAU  - Smith, J\nPY  - 2020\nER  - \nTY  - JOUR\nTI  - Second\nER  - \n";
        let result = parse_ris(input, &ParseOptions::default());
        let titles: Vec<_> = result.entries.iter().map(|e| e["title"].clone()).collect();
        assert_eq!(titles, vec![json!("First"), json!("Second")]);
        assert_eq!(result.entries[0]["author"][0]["family"], "Smith");
    }

    #[test]
    fn test_parse_ris_ambiguous_da_is_not_a_year() {
        // "05/12/2019" is day/month or month/day; reading its first number
        // as the year imported a 2019 article as year 5 over a correct PY.
        let issued = |input: &str| {
            parse_ris(input, &ParseOptions::default()).entries[0]["issued"]["date-parts"][0].clone()
        };
        assert_eq!(
            issued("TY  - JOUR\nPY  - 2019\nDA  - 05/12/2019\nER  - "),
            json!([2019])
        );
        assert_eq!(
            issued("TY  - JOUR\nDA  - 05/12/2019\nPY  - 2019/05/12/\nER  - "),
            json!([2019, 5, 12])
        );
        // Without a PY, the trailing four-digit year is all that is certain.
        assert_eq!(
            issued("TY  - JOUR\nDA  - 05/12/2019\nER  - "),
            json!([2019])
        );
        // A year-first DA still carries the date proper.
        assert_eq!(
            issued("TY  - JOUR\nDA  - 2019/12/05/\nPY  - 2019\nER  - "),
            json!([2019, 12, 5])
        );
    }

    #[test]
    fn test_parse_ris_sn_files_issns_as_issn() {
        // `SN` holds an ISSN for serials and an ISBN for books. Only the
        // bare hyphenated form was recognized, so PubMed-style values with a
        // qualifier or without the hyphen became ISBNs.
        let sn = |value: &str| {
            let input = format!("TY  - JOUR\nTI  - T\nSN  - {value}\nER  - ");
            let entry = parse_ris(&input, &ParseOptions::default()).entries[0].clone();
            (entry["ISSN"].clone(), entry["ISBN"].clone())
        };
        assert_eq!(
            sn("1476-4687 (Electronic)"),
            (json!("1476-4687"), Value::Null)
        );
        assert_eq!(sn("0028-0836 (Print)"), (json!("0028-0836"), Value::Null));
        assert_eq!(sn("1234-567X (Linking)"), (json!("1234-567X"), Value::Null));
        assert_eq!(sn("00280836"), (json!("00280836"), Value::Null));
        assert_eq!(sn("1476-4687"), (json!("1476-4687"), Value::Null));
        assert_eq!(
            sn("978-3-16-148410-0"),
            (Value::Null, json!("978-3-16-148410-0"))
        );
        assert_eq!(sn("0306406152"), (Value::Null, json!("0306406152")));
    }

    #[test]
    fn test_parse_ris_wrapped_lines_continue_their_field() {
        // A line without a tag continues the field above it. Skipping it
        // truncated wrapped titles and abstracts.
        let input = "TY  - JOUR\nTI  - A very long title that the exporter\n      wrapped onto a second line\nAU  - Smith, J\nAB  - First sentence.\nSecond sentence.\nT2  - Journal of\n  Long Names\nER  - \n";
        let result = parse_ris(input, &ParseOptions::default());
        let entry = &result.entries[0];
        assert_eq!(
            entry["title"],
            "A very long title that the exporter wrapped onto a second line"
        );
        assert_eq!(entry["abstract"], "First sentence. Second sentence.");
        assert_eq!(entry["container-title"], "Journal of Long Names");
        assert_eq!(entry["author"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_ris_untagged_lines_under_values_are_not_wrapped_text() {
        // EndNote writes one value per line inside a field. A second URL is
        // not the rest of the first one, and a second keyword is a keyword.
        let input = "TY  - JOUR\nTI  - T\nUR  - https://a.example/x\nhttps://b.example/y\nDO  - 10.1/abc\n10.2/def\nKW  - alpha\nbeta\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        let entry = &result.entries[0];
        assert_eq!(entry["URL"], "https://a.example/x");
        assert_eq!(entry["DOI"], "10.1/abc");
        assert_eq!(entry["keyword"], "alpha, beta");
    }

    #[test]
    fn test_parse_ris_lowercase_words_before_a_dash_are_not_tags() {
        // RIS tags are uppercase; "and  - " inside wrapped prose is text.
        let input = "TY  - JOUR\nTI  - Before\n  and  - after\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["title"], "Before and  - after");
    }

    #[test]
    fn test_parse_ris_edition_and_patent() {
        let input = "TY  - BOOK\nTI  - T\nET  - 2nd\nER  - \nTY  - PAT\nTI  - A Device\nER  - ";
        let result = parse_ris(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["edition"], "2nd");
        assert_eq!(result.entries[1]["type"], "patent");
    }

    #[test]
    fn test_parse_ris_editor_dedup_is_linear() {
        // Every ED was compared against every A2: 20k of each took ~6 s in a
        // release build. A 5k pair is ~25M comparisons when quadratic.
        let n = 5_000;
        let mut input = String::from("TY  - CHAP\nTI  - T\n");
        for i in 0..n {
            input.push_str(&format!("A2  - A{i}, B\n"));
        }
        for i in 0..n {
            input.push_str(&format!("ED  - E{i}, F\n"));
        }
        input.push_str("ED  - A0, B\nER  - \n");
        let started = std::time::Instant::now();
        let result = parse_ris(&input, &ParseOptions::default());
        let elapsed = started.elapsed();
        assert_eq!(
            result.entries[0]["editor"].as_array().unwrap().len(),
            2 * n,
            "the ED repeating an A2 is listed once"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "{n} editors took {elapsed:?}"
        );
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
