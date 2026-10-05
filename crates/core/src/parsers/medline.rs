use super::{ParseErrorInfo, ParseOptions, ParseResult};
use serde_json::{json, Value};

/// Parse MEDLINE/NBIB content into CSL-JSON items.
///
/// MEDLINE/NBIB is a tagged format from PubMed/NLM. Each record is separated
/// by a blank line. Tags are 4 characters wide, left-justified, followed by "- ".
///
/// Key tags:
/// - PMID: PubMed ID
/// - PMC: PubMed Central ID ("PMC6500000")
/// - TI: Title
/// - FAU: Full author name ("Last, First Middle")
/// - AU: Abbreviated author ("Last FM")
/// - DP: Date of publication ("2024 Mar" or "2024")
/// - TA: Journal abbreviation
/// - JT: Full journal title
/// - VI: Volume
/// - IP: Issue/Part
/// - PG: Pages
/// - LID: DOI (ends with " [doi]")
/// - AID: Alternate ID — also DOI when ends with " [doi]"
/// - AB: Abstract
/// - LA: Language
/// - PT: Publication type
/// - IS: ISSN
/// - PL: Place of publication
pub fn parse_medline(input: &str, options: &ParseOptions) -> ParseResult {
    if input.trim().is_empty() {
        return ParseResult {
            entries: vec![],
            errors: vec![],
            format: "medline".to_string(),
            truncated: false,
            scanned_entries: 0,
        };
    }

    // DoS guard
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
            format: "medline".to_string(),
            truncated: true,
            scanned_entries: 0,
        };
    }

    let mut entries: Vec<Value> = Vec::new();
    let mut current: Option<MedlineRecord> = None;
    let mut scanned = 0;
    let mut last_tag = String::new();
    // Indent of the most recent tag line: column 0 in PubMed output, deeper
    // when a record was pasted indented.
    let mut tag_indent: Option<usize> = None;

    for line in input.lines() {
        // A byte-order mark (Windows tools write one) is not part of the
        // first tag; left in place, "PMID" stopped being a tag.
        let line = line.trim_start_matches('\u{FEFF}');

        // Blank line = record separator. The next record sets its own tag
        // indent: carrying this one over made every line of a record pasted
        // deeper than the one before read as a wrapped line, and the record
        // vanished.
        if line.trim().is_empty() {
            if let Some(record) = current.take() {
                if !push_record(&mut entries, record, &mut scanned, options.max_entries) {
                    return truncated_result(entries, scanned);
                }
            }
            last_tag.clear();
            tag_indent = None;
            continue;
        }

        // A wrapped line sits deeper than the tag lines (six spaces in
        // PubMed's output). That check has to come before tag parsing: a
        // wrapped line opening with "HIV-1" or "IL-6" otherwise reads as tag
        // "HIV"/"IL", which cuts the field short and files the rest under a
        // tag nobody reads. It is relative to the tag indent, not a fixed six
        // spaces, so a record pasted with an indent still parses and a
        // normalizer that shrank the indent still wraps. Only an open record
        // has a field to continue, and a PMID always opens a record.
        let indent = line.len() - line.trim_start().len();
        let parsed = parse_tag_line(line);
        let opens_record = parsed.as_ref().is_some_and(|(tag, _)| tag == "PMID");
        if current.is_some() && !opens_record && tag_indent.is_some_and(|tags| indent > tags) {
            if let Some(ref mut record) = current {
                record.continue_field(&last_tag, line.trim());
            }
            continue;
        }

        // MEDLINE tags are commonly fixed-width (`TI  - value`), but text
        // normalizers often collapse the double spaces to `TI - value`.
        let Some((tag, value)) = parsed else {
            // Not a tag: a wrapped line whose indent a paste stripped. It
            // still belongs to the field above it.
            if let Some(ref mut record) = current {
                record.continue_field(&last_tag, line.trim());
            }
            continue;
        };
        tag_indent = Some(indent);

        // A PMID inside a record that already has one means the blank line
        // between two records was lost; without this, both merge into one.
        if tag == "PMID" && current.as_ref().is_some_and(|r| r.pmid.is_some()) {
            if let Some(record) = current.take() {
                if !push_record(&mut entries, record, &mut scanned, options.max_entries) {
                    return truncated_result(entries, scanned);
                }
            }
        }

        // Start a record on PMID, or on a title/author line for exports
        // that lack PMID.
        if current.is_none() && matches!(tag.as_str(), "PMID" | "TI" | "FAU" | "AU") {
            current = Some(MedlineRecord::new());
        }

        if let Some(ref mut record) = current {
            record.add_field(&tag, &value);
        }

        last_tag = tag;
    }

    // Handle last record (no trailing blank line)
    let mut truncated = false;
    if let Some(record) = current.take() {
        truncated = !push_record(&mut entries, record, &mut scanned, options.max_entries);
    }

    ParseResult {
        entries,
        errors: vec![],
        format: "medline".to_string(),
        truncated,
        scanned_entries: scanned,
    }
}

/// Count a finished record and keep it unless `max_entries` is reached.
/// Returns `false` when the record was dropped for the limit.
fn push_record(
    entries: &mut Vec<Value>,
    record: MedlineRecord,
    scanned: &mut usize,
    max_entries: Option<usize>,
) -> bool {
    *scanned += 1;
    if max_entries.is_some_and(|max| entries.len() >= max) {
        return false;
    }
    entries.push(record.to_csl_json());
    true
}

fn truncated_result(entries: Vec<Value>, scanned: usize) -> ParseResult {
    ParseResult {
        entries,
        errors: vec![],
        format: "medline".to_string(),
        truncated: true,
        scanned_entries: scanned,
    }
}

/// Tags `MedlineRecord::add_field` reads.
const READ_TAGS: &[&str] = &[
    "PMID", "PMC", "TI", "FAU", "AU", "CN", "DP", "TA", "JT", "VI", "IP", "PG", "LID", "AID", "AB",
    "LA", "PT", "IS", "PL", "OT",
];

fn parse_tag_line(line: &str) -> Option<(String, String)> {
    let (raw_tag, raw_value) = line.split_once('-')?;
    let tag = raw_tag.trim();
    // Real tag lines put a space after the dash ("TI  - x", "PMID- 1") or end
    // there. Normalizers sometimes eat that space, so the squeezed form is
    // accepted — but only for tags we read, which keeps a de-indented wrapped
    // line like "HIV-1 infection" from reading as tag "HIV".
    let spaced = raw_value.is_empty() || raw_value.starts_with(char::is_whitespace);
    if !spaced && !READ_TAGS.contains(&tag) {
        return None;
    }
    if !(2..=4).contains(&tag.len()) {
        return None;
    }
    if !tag
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
    {
        return None;
    }
    Some((tag.to_string(), raw_value.trim_start().to_string()))
}

/// One author slot, in source order.
///
/// PubMed writes each person as `FAU` (full) followed by its `AU`
/// abbreviation; records indexed before 2002 carry only `AU`. `CN` is a
/// corporate author and takes its place in the list like any other.
enum MedlineAuthor {
    Person {
        full: Option<String>,
        abbreviated: Option<String>,
    },
    Corporate(String),
}

impl MedlineAuthor {
    fn to_csl_json(&self) -> Option<Value> {
        match self {
            MedlineAuthor::Person {
                full: Some(fau), ..
            } => {
                // FAU format: "Last, First Middle"
                let parts: Vec<&str> = fau.splitn(2, ',').collect();
                Some(if parts.len() == 2 {
                    json!({"family": parts[0].trim(), "given": parts[1].trim()})
                } else {
                    json!({"literal": fau})
                })
            }
            MedlineAuthor::Person {
                full: None,
                abbreviated: Some(au),
            } => Some(abbreviated_author(au)),
            MedlineAuthor::Person { .. } => None,
            MedlineAuthor::Corporate(name) => Some(json!({"literal": name})),
        }
    }
}

/// `AU` form: "Smith JA" → family "Smith", given "J. A.". The token after
/// the family name is the initials; anything before it (particles included)
/// is the family name, and a generational suffix may follow ("Smith JA Jr").
/// Initials are spelled out with periods because CSL processors initialize
/// per word — a bare "JA" would render as "J.".
fn abbreviated_author(au: &str) -> Value {
    let mut tokens: Vec<&str> = au.split_whitespace().collect();
    // Three tokens at least: in "Vasquez IV" the "IV" is initials.
    let suffix = if tokens.len() >= 3
        && tokens.last().is_some_and(|t| {
            ["Jr", "Jr.", "Sr", "Sr.", "II", "III", "IV", "2nd", "3rd"].contains(t)
        }) {
        tokens.pop()
    } else {
        None
    };
    match tokens.split_last() {
        Some((initials, family))
            if !family.is_empty() && initials.chars().all(|c| c.is_uppercase()) =>
        {
            let given = initials
                .chars()
                .map(|c| format!("{c}."))
                .collect::<Vec<_>>()
                .join(" ");
            let mut name = json!({"family": family.join(" "), "given": given});
            if let Some(suffix) = suffix {
                name["suffix"] = json!(suffix);
            }
            name
        }
        _ => json!({"literal": au}),
    }
}

fn push_wrapped(target: &mut String, text: &str) {
    target.push(' ');
    target.push_str(text);
}

/// Append a wrapped line's text to a single-valued field.
fn append_to(slot: &mut Option<String>, text: &str) {
    match slot {
        Some(existing) => push_wrapped(existing, text),
        None => *slot = Some(text.to_string()),
    }
}

fn append_to_last(list: &mut [String], text: &str) {
    if let Some(last) = list.last_mut() {
        push_wrapped(last, text);
    }
}

/// Internal record accumulator.
struct MedlineRecord {
    pmid: Option<String>,
    pmcid: Option<String>,
    title: Option<String>,
    authors: Vec<MedlineAuthor>,
    date: Option<String>,         // DP: "2024 Mar" or "2024"
    journal_abbr: Option<String>, // TA
    journal_full: Option<String>, // JT
    volume: Option<String>,
    issue: Option<String>,
    pages: Option<String>,
    doi: Option<String>,
    abstract_text: Option<String>,
    language: Option<String>,
    pub_types: Vec<String>,
    issn: Option<String>,
    place: Option<String>,
    keywords: Vec<String>,
}

impl MedlineRecord {
    fn new() -> Self {
        Self {
            pmid: None,
            pmcid: None,
            title: None,
            authors: Vec::new(),
            date: None,
            journal_abbr: None,
            journal_full: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            abstract_text: None,
            language: None,
            pub_types: Vec::new(),
            issn: None,
            place: None,
            keywords: Vec::new(),
        }
    }

    fn add_field(&mut self, tag: &str, value: &str) {
        match tag {
            "PMID" => self.pmid = Some(value.to_string()),
            "PMC" => self.pmcid = Some(value.to_string()),
            // A repeated TI/AB continues the field rather than replacing it.
            "TI" => append_to(&mut self.title, value),
            "FAU" => self.authors.push(MedlineAuthor::Person {
                full: Some(value.to_string()),
                abbreviated: None,
            }),
            "AU" => match self.authors.last_mut() {
                // The abbreviation of the FAU just before it, not a new author.
                Some(MedlineAuthor::Person {
                    full: Some(_),
                    abbreviated: slot @ None,
                }) => *slot = Some(value.to_string()),
                _ => self.authors.push(MedlineAuthor::Person {
                    full: None,
                    abbreviated: Some(value.to_string()),
                }),
            },
            "CN" => self
                .authors
                .push(MedlineAuthor::Corporate(value.to_string())),
            "DP" => self.date = Some(value.to_string()),
            "TA" => self.journal_abbr = Some(value.to_string()),
            "JT" => self.journal_full = Some(value.to_string()),
            "VI" => self.volume = Some(value.to_string()),
            "IP" => self.issue = Some(value.to_string()),
            "PG" => self.pages = Some(value.to_string()),
            "LID" | "AID" => {
                // Extract DOI: "10.1234/test.2024 [doi]"
                if value.ends_with("[doi]") && self.doi.is_none() {
                    self.doi = Some(value.trim_end_matches("[doi]").trim().to_string());
                }
            }
            "AB" => append_to(&mut self.abstract_text, value),
            "LA" => self.language = Some(value.to_string()),
            "PT" => self.pub_types.push(value.to_string()),
            "IS" => {
                // First ISSN wins (Electronic preferred)
                if self.issn.is_none() {
                    // "1234-5678 (Electronic)" → "1234-5678"
                    self.issn = Some(value.split_whitespace().next().unwrap_or(value).to_string());
                }
            }
            "PL" => self.place = Some(value.to_string()),
            "OT" => self.keywords.push(value.to_string()),
            _ => {} // Skip unknown tags
        }
    }

    /// Append a wrapped line to whatever field `tag` last wrote. Routing the
    /// line back through `add_field` would *replace* single-valued fields
    /// (the journal became its last fragment) and *add* list items (a
    /// wrapped name became a second author).
    fn continue_field(&mut self, tag: &str, text: &str) {
        match tag {
            "TI" => append_to(&mut self.title, text),
            "AB" => append_to(&mut self.abstract_text, text),
            "JT" => append_to(&mut self.journal_full, text),
            "TA" => append_to(&mut self.journal_abbr, text),
            "PL" => append_to(&mut self.place, text),
            "DP" => append_to(&mut self.date, text),
            "PT" => append_to_last(&mut self.pub_types, text),
            "OT" => append_to_last(&mut self.keywords, text),
            "FAU" | "AU" | "CN" => match (tag, self.authors.last_mut()) {
                (
                    "FAU",
                    Some(MedlineAuthor::Person {
                        full: Some(name), ..
                    }),
                )
                | (
                    "AU",
                    Some(MedlineAuthor::Person {
                        abbreviated: Some(name),
                        ..
                    }),
                )
                | ("CN", Some(MedlineAuthor::Corporate(name))) => push_wrapped(name, text),
                _ => {}
            },
            _ => {} // Identifiers and unknown tags don't wrap meaningfully
        }
    }

    fn to_csl_json(&self) -> Value {
        let mut obj = serde_json::Map::new();

        // Type: derive from PT tags
        let csl_type = self.derive_type();
        obj.insert("type".into(), json!(csl_type));

        // ID: use PMID or generate
        if let Some(ref pmid) = self.pmid {
            obj.insert("id".into(), json!(format!("PMID:{pmid}")));
            obj.insert("PMID".into(), json!(pmid));
        }
        if let Some(ref pmcid) = self.pmcid {
            obj.insert("PMCID".into(), json!(pmcid));
        }

        // Title
        if let Some(ref title) = self.title {
            obj.insert("title".into(), json!(title));
        }

        // Authors — full names when present, abbreviations otherwise
        let names: Vec<Value> = self
            .authors
            .iter()
            .filter_map(MedlineAuthor::to_csl_json)
            .collect();
        if !names.is_empty() {
            obj.insert("author".into(), json!(names));
        }

        // Date from DP: "2024 Mar 15", "2024 Mar", "2023 Jan-Feb" or "2024"
        if let Some(date_parts) = self.date.as_deref().and_then(parse_dp) {
            obj.insert("issued".into(), json!({"date-parts": [date_parts]}));
        }

        // Journal: prefer full title, fallback to abbreviation
        if let Some(ref jt) = self.journal_full {
            obj.insert("container-title".into(), json!(jt));
        } else if let Some(ref ta) = self.journal_abbr {
            obj.insert("container-title".into(), json!(ta));
        }
        if let Some(ref ta) = self.journal_abbr {
            obj.insert("container-title-short".into(), json!(ta));
        }

        // Biblio fields
        if let Some(ref v) = self.volume {
            obj.insert("volume".into(), json!(v));
        }
        if let Some(ref v) = self.issue {
            obj.insert("issue".into(), json!(v));
        }
        if let Some(ref v) = self.pages {
            obj.insert("page".into(), json!(v));
        }
        if let Some(ref v) = self.doi {
            obj.insert("DOI".into(), json!(v));
        }
        if let Some(ref v) = self.abstract_text {
            obj.insert("abstract".into(), json!(v));
        }
        if let Some(ref v) = self.language {
            obj.insert("language".into(), json!(v));
        }
        if let Some(ref v) = self.issn {
            obj.insert("ISSN".into(), json!(v));
        }
        if let Some(ref v) = self.place {
            obj.insert("publisher-place".into(), json!(v));
        }

        // Keywords
        if !self.keywords.is_empty() {
            obj.insert("keyword".into(), json!(self.keywords.join(", ")));
        }

        Value::Object(obj)
    }

    fn derive_type(&self) -> &'static str {
        for pt in &self.pub_types {
            let lower = pt.to_lowercase();
            if lower.contains("review") {
                return "article-journal";
            }
            if lower.contains("book") {
                return "book";
            }
            if lower.contains("congress") || lower.contains("conference") {
                return "paper-conference";
            }
            if lower.contains("dataset") {
                return "dataset";
            }
        }
        "article-journal" // Default for MEDLINE
    }
}

/// `DP` → CSL date-parts. The day is kept when it is a valid day of a
/// known month; a month range ("Jan-Feb") keeps its first month; seasons
/// and other text fall back to the year.
fn parse_dp(dp: &str) -> Option<Vec<i32>> {
    let mut parts = dp.split_whitespace();
    let year = parts.next()?.parse::<i32>().ok()?;
    let mut date_parts = vec![year];
    let month = parts
        .next()
        .and_then(|m| month_to_number(m.split('-').next().unwrap_or(m)));
    if let Some(m) = month {
        date_parts.push(m);
        if let Some(day) = parts
            .next()
            .and_then(|d| d.parse::<i32>().ok())
            .filter(|d| (1..=31).contains(d))
        {
            date_parts.push(day);
        }
    }
    Some(date_parts)
}

/// Convert 3-letter month abbreviation to number.
fn month_to_number(month: &str) -> Option<i32> {
    match month.to_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_medline_basic() {
        let input = include_str!("../../../../tests/fixtures/samples/sample.nbib");
        let result = parse_medline(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            2,
            "should parse 2 entries: {:?}",
            result.errors
        );
        assert!(result.errors.is_empty());
        assert_eq!(result.format, "medline");

        let first = &result.entries[0];
        assert_eq!(
            first["title"],
            "A Study of Something Important in Modern Medicine"
        );
        assert_eq!(first["DOI"], "10.1234/test.2024");
        assert_eq!(first["PMID"], "12345678");
        assert_eq!(first["volume"], "42");
        assert_eq!(first["issue"], "3");
        assert_eq!(first["page"], "100-115");
        assert_eq!(first["language"], "eng");
        assert_eq!(first["container-title"], "Journal of Testing");

        // Check authors
        let authors = first["author"].as_array().unwrap();
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0]["family"], "Smith");
        assert_eq!(authors[0]["given"], "John A");

        // Check date with month
        let date_parts = &first["issued"]["date-parts"][0];
        assert_eq!(date_parts[0], 2024);
        assert_eq!(date_parts[1], 3); // March

        // Second entry
        let second = &result.entries[1];
        assert_eq!(second["title"], "Advances in Quantum Computing: A Review");
        assert_eq!(second["container-title"], "Nature Reviews");
    }

    #[test]
    fn test_parse_medline_accepts_collapsed_tag_spacing() {
        let input = r#"PMID- 41764257
TI - A normalized MEDLINE title.
FAU - Zaman, Khalid
DP - 2026 Feb
JT - Scientific reports
LID - 10.1038/s41598-026-40798-8 [doi]

"#;
        let result = parse_medline(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "should parse normalized tags: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["title"], "A normalized MEDLINE title.");
        assert_eq!(first["container-title"], "Scientific reports");
        assert_eq!(first["DOI"], "10.1038/s41598-026-40798-8");
    }

    #[test]
    fn test_parse_medline_empty() {
        let result = parse_medline("", &ParseOptions::default());
        assert!(result.entries.is_empty());
    }

    #[test]
    fn test_parse_medline_max_entries() {
        let input = include_str!("../../../../tests/fixtures/samples/sample.nbib");
        let opts = ParseOptions {
            max_entries: Some(1),
            ..Default::default()
        };
        let result = parse_medline(input, &opts);
        assert_eq!(result.entries.len(), 1);
        assert!(result.truncated);
    }

    #[test]
    fn test_parse_medline_wrapped_lines_continue_their_field() {
        // PubMed wraps at ~80 columns with a six-space indent. A wrapped line
        // opening with "HIV-1" used to parse as tag "HIV", truncating the
        // title and filing the rest under that bogus tag; a wrapped `JT`
        // re-set the journal to its last fragment — PNAS came out as
        // "America".
        let input = "PMID- 1\n\
TI  - Effects of antiretroviral therapy on outcomes in patients with\n      \
HIV-1 infection and tuberculosis coinfection in rural settings.\n\
FAU - Smith, John\n\
JT  - Proceedings of the National Academy of Sciences of the United States of\n      \
America\n\
TA  - Proc Natl Acad Sci U S A\n";
        let result = parse_medline(input, &ParseOptions::default());
        let first = &result.entries[0];
        assert_eq!(
            first["title"],
            "Effects of antiretroviral therapy on outcomes in patients with HIV-1 \
             infection and tuberculosis coinfection in rural settings."
        );
        assert_eq!(
            first["container-title"],
            "Proceedings of the National Academy of Sciences of the United States of America"
        );
        assert_eq!(first["container-title-short"], "Proc Natl Acad Sci U S A");
    }

    #[test]
    fn test_parse_medline_dash_inside_value_is_not_a_tag() {
        // A de-indented wrapped line must not become a tag either: real tag
        // lines always put a space (or end of line) after the dash.
        assert_eq!(parse_tag_line("HIV-1 infection and tuberculosis"), None);
        assert_eq!(parse_tag_line("IL-6 levels"), None);
        assert_eq!(
            parse_tag_line("PMID- 123"),
            Some(("PMID".to_string(), "123".to_string()))
        );
        assert_eq!(
            parse_tag_line("TI  - Long-term outcomes"),
            Some(("TI".to_string(), "Long-term outcomes".to_string()))
        );
    }

    #[test]
    fn test_parse_medline_abbreviated_authors_when_no_fau() {
        // Records indexed before 2002 carry only `AU` ("Smith JA"); reading
        // only `FAU` imported them with no authors at all.
        let input = "PMID- 2\nTI  - Old paper.\nAU  - Smith JA\nAU  - van der Berg K\nCN  - WHO Study Group\nDP  - 1985\n";
        let result = parse_medline(input, &ParseOptions::default());
        let authors = result.entries[0]["author"].as_array().expect("authors");
        assert_eq!(authors.len(), 3, "{authors:?}");
        assert_eq!(authors[0]["family"], "Smith");
        assert_eq!(authors[0]["given"], "J. A.");
        assert_eq!(authors[1]["family"], "van der Berg");
        assert_eq!(authors[1]["given"], "K.");
        assert_eq!(authors[2]["literal"], "WHO Study Group");
    }

    #[test]
    fn test_parse_medline_full_names_win_and_keep_order() {
        // With FAU present, each AU is the abbreviation of the FAU before it
        // and must not add a second copy of the author.
        let input = "PMID- 3\nFAU - Smith, John A\nAU  - Smith JA\nCN  - Trial Group\nFAU - Doe, Jane\nAU  - Doe J\n";
        let result = parse_medline(input, &ParseOptions::default());
        let authors = result.entries[0]["author"].as_array().expect("authors");
        assert_eq!(authors.len(), 3, "{authors:?}");
        assert_eq!(authors[0]["given"], "John A");
        assert_eq!(authors[1]["literal"], "Trial Group");
        assert_eq!(authors[2]["family"], "Doe");
    }

    #[test]
    fn test_parse_medline_pmid_starts_a_new_record() {
        // Records pasted without their blank-line separators used to merge:
        // one entry, the second PMID, both titles concatenated.
        let input = "PMID- 3\nTI  - First.\nFAU - A, B\nPMID- 4\nTI  - Second.\nFAU - C, D\n";
        let result = parse_medline(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 2, "{:?}", result.entries);
        assert_eq!(result.entries[0]["PMID"], "3");
        assert_eq!(result.entries[0]["title"], "First.");
        assert_eq!(result.entries[1]["PMID"], "4");
        assert_eq!(result.entries[1]["author"][0]["family"], "C");
    }

    #[test]
    fn test_parse_medline_date_keeps_day_and_first_month_of_range() {
        let parse = |dp: &str| {
            let input = format!("PMID- 5\nTI  - T.\nDP  - {dp}\n");
            parse_medline(&input, &ParseOptions::default()).entries[0]["issued"]["date-parts"][0]
                .clone()
        };
        assert_eq!(parse("2020 Mar 15"), serde_json::json!([2020, 3, 15]));
        assert_eq!(parse("2023 Jan-Feb"), serde_json::json!([2023, 1]));
        assert_eq!(parse("2024"), serde_json::json!([2024]));
        assert_eq!(parse("2024 Mar 99"), serde_json::json!([2024, 3]));
    }

    #[test]
    fn test_parse_medline_uniformly_indented_file_still_parses() {
        // A whole record pasted with an indent: continuation means "more
        // indented than the tag lines", not "starts with six spaces".
        let input = "      PMID- 9\n      TI  - Indented title that wraps\n            onto a second line.\n      FAU - Smith, John\n";
        let result = parse_medline(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1, "{:?}", result.errors);
        let first = &result.entries[0];
        assert_eq!(first["PMID"], "9");
        assert_eq!(
            first["title"],
            "Indented title that wraps onto a second line."
        );
        assert_eq!(first["author"][0]["family"], "Smith");
    }

    #[test]
    fn test_parse_medline_known_tags_without_space_after_dash() {
        // Normalizers sometimes eat the space after the dash. Known tags are
        // still tags; an unknown word is not (that is the "HIV-1" guard).
        let input = "PMID-123\nTI  -Squeezed title.\n";
        let result = parse_medline(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["PMID"], "123");
        assert_eq!(result.entries[0]["title"], "Squeezed title.");
        assert_eq!(parse_tag_line("HIV-1 infection"), None);
    }

    #[test]
    fn test_parse_medline_abbreviated_author_with_suffix() {
        let input = "PMID- 6\nAU  - Smith JA Jr\n";
        let result = parse_medline(input, &ParseOptions::default());
        assert_eq!(
            result.entries[0]["author"][0],
            serde_json::json!({"family": "Smith", "given": "J. A.", "suffix": "Jr"})
        );
    }

    #[test]
    fn test_parse_medline_record_indented_deeper_than_the_one_before() {
        // The tag indent carried over the blank line, so every line of a
        // more-indented second record read as a continuation of nothing and
        // the record vanished (0.3.8 read both).
        let input =
            "PMID- 1\nTI  - First.\nFAU - A, B\n\n  PMID- 2\n  TI  - Second.\n  FAU - C, D\n";
        let result = parse_medline(input, &ParseOptions::default());
        let titles: Vec<_> = result.entries.iter().map(|e| e["title"].clone()).collect();
        assert_eq!(
            titles,
            vec![serde_json::json!("First."), serde_json::json!("Second.")]
        );
        assert_eq!(result.entries[1]["PMID"], "2");
        assert_eq!(result.entries[1]["author"][0]["family"], "C");
    }

    #[test]
    fn test_parse_medline_deindented_wrapped_line_continues_its_field() {
        // A paste that stripped the six-space indent leaves the wrapped line
        // at the tag column. It can't be a tag, so it continues the field —
        // dropping it cut the title short.
        let input = "PMID- 1\nTI  - Effects of therapy on outcomes in patients with\nsevere disease and comorbidities.\nFAU - Smith, John\nAB  - Background text\nHIV-1 infection rates rose.\nDP  - 2020\n";
        let result = parse_medline(input, &ParseOptions::default());
        let first = &result.entries[0];
        assert_eq!(
            first["title"],
            "Effects of therapy on outcomes in patients with severe disease and comorbidities."
        );
        assert_eq!(
            first["abstract"],
            "Background text HIV-1 infection rates rose."
        );
        assert_eq!(first["author"].as_array().unwrap().len(), 1);
        assert_eq!(first["issued"]["date-parts"][0], serde_json::json!([2020]));
    }

    #[test]
    fn test_parse_medline_pmc_is_pmcid() {
        let input = "PMID- 31000000\nTI  - T.\nFAU - A, B\nDP  - 2019\nPMC - PMC6500000\n";
        let result = parse_medline(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["PMCID"], "PMC6500000");
    }

    #[test]
    fn test_parse_medline_utf8_bom_keeps_the_pmid() {
        let input = "\u{FEFF}PMID- 7\nTI  - T.\nFAU - A, B\n";
        let result = parse_medline(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0]["PMID"], "7");
    }

    #[test]
    fn test_month_conversion() {
        assert_eq!(month_to_number("Mar"), Some(3));
        assert_eq!(month_to_number("dec"), Some(12));
        assert_eq!(month_to_number("xyz"), None);
    }
}
