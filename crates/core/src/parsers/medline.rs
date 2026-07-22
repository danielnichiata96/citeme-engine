use super::{ParseErrorInfo, ParseOptions, ParseResult};
use serde_json::{json, Value};

/// Parse MEDLINE/NBIB content into CSL-JSON items.
///
/// MEDLINE/NBIB is a tagged format from PubMed/NLM. Each record is separated
/// by a blank line. Tags are 4 characters wide, left-justified, followed by "- ".
///
/// Key tags:
/// - PMID: PubMed ID
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

    for line in input.lines() {
        // Blank line = record separator
        if line.trim().is_empty() {
            if let Some(record) = current.take() {
                scanned += 1;
                if let Some(max) = options.max_entries {
                    if entries.len() >= max {
                        return ParseResult {
                            entries,
                            errors: vec![],
                            format: "medline".to_string(),
                            truncated: true,
                            scanned_entries: scanned,
                        };
                    }
                }
                entries.push(record.to_csl_json());
            }
            last_tag.clear();
            continue;
        }

        // MEDLINE tags are commonly fixed-width (`TI  - value`), but text
        // normalizers often collapse the double spaces to `TI - value`.
        let (tag, value) = if let Some((tag, value)) = parse_tag_line(line) {
            (tag, value)
        } else if line.starts_with("      ") {
            // Continuation line (6+ spaces) — append to last tag
            (last_tag.clone(), line.trim().to_string())
        } else {
            continue;
        };

        if tag.is_empty() {
            continue;
        }

        // Start new record on PMID
        if tag == "PMID" && current.is_none() {
            current = Some(MedlineRecord::new());
        }

        // If no record started yet but we see a TI, start one (some exports lack PMID)
        if current.is_none() && (tag == "TI" || tag == "FAU" || tag == "AU") {
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
        scanned += 1;
        if options.max_entries.is_none_or(|max| entries.len() < max) {
            entries.push(record.to_csl_json());
        } else {
            truncated = true;
        }
    }

    ParseResult {
        entries,
        errors: vec![],
        format: "medline".to_string(),
        truncated,
        scanned_entries: scanned,
    }
}

fn parse_tag_line(line: &str) -> Option<(String, String)> {
    let (raw_tag, raw_value) = line.split_once('-')?;
    let tag = raw_tag.trim();
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

/// Internal record accumulator.
struct MedlineRecord {
    pmid: Option<String>,
    title: Option<String>,
    authors: Vec<String>,         // FAU names: "Last, First Middle"
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
            "TI" => {
                // TI can span multiple continuation lines
                if let Some(ref mut t) = self.title {
                    t.push(' ');
                    t.push_str(value);
                } else {
                    self.title = Some(value.to_string());
                }
            }
            "FAU" => self.authors.push(value.to_string()),
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
            "AB" => {
                if let Some(ref mut a) = self.abstract_text {
                    a.push(' ');
                    a.push_str(value);
                } else {
                    self.abstract_text = Some(value.to_string());
                }
            }
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

        // Title
        if let Some(ref title) = self.title {
            obj.insert("title".into(), json!(title));
        }

        // Authors (from FAU — full author names)
        if !self.authors.is_empty() {
            let names: Vec<Value> = self
                .authors
                .iter()
                .map(|fau| {
                    // FAU format: "Last, First Middle"
                    let parts: Vec<&str> = fau.splitn(2, ',').collect();
                    if parts.len() == 2 {
                        json!({"family": parts[0].trim(), "given": parts[1].trim()})
                    } else {
                        json!({"literal": fau})
                    }
                })
                .collect();
            obj.insert("author".into(), json!(names));
        }

        // Date from DP: "2024 Mar" or "2024"
        if let Some(ref dp) = self.date {
            let parts: Vec<&str> = dp.split_whitespace().collect();
            if let Some(year_str) = parts.first() {
                if let Ok(year) = year_str.parse::<i32>() {
                    let month = parts.get(1).and_then(|m| month_to_number(m));
                    let mut date_parts: Vec<i32> = vec![year];
                    if let Some(m) = month {
                        date_parts.push(m);
                    }
                    obj.insert("issued".into(), json!({"date-parts": [date_parts]}));
                }
            }
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
    fn test_month_conversion() {
        assert_eq!(month_to_number("Mar"), Some(3));
        assert_eq!(month_to_number("dec"), Some(12));
        assert_eq!(month_to_number("xyz"), None);
    }
}
