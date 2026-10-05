use std::borrow::Cow;

use super::bibtex_guard::{self, EntryNotes};
use super::{ParseErrorInfo, ParseOptions, ParseResult, MAX_PARSE_ERRORS};
use biblatex::{ChunksExt, DateValue, PermissiveType, TypeError, TypeErrorKind};
use hayagriva::Entry;
use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;

fn combining_latex_command(mark: char) -> Option<&'static str> {
    match mark {
        '\u{300}' => Some("`"),
        '\u{301}' => Some("'"),
        '\u{302}' => Some("^"),
        '\u{303}' => Some("~"),
        '\u{304}' => Some("="),
        '\u{306}' => Some("u"),
        '\u{307}' => Some("."),
        '\u{308}' => Some("\""),
        '\u{30A}' => Some("r"),
        '\u{30B}' => Some("H"),
        '\u{30C}' => Some("v"),
        '\u{323}' => Some("d"),
        '\u{327}' => Some("c"),
        '\u{328}' => Some("k"),
        '\u{332}' => Some("b"),
        '\u{338}' => Some("o"),
        _ => None,
    }
}

fn normalize_bibtex_input(input: &str) -> Cow<'_, str> {
    let mut normalized = None;
    let mut chars = input.char_indices().peekable();
    let mut last = 0;

    while let Some((idx, ch)) = chars.next() {
        if ch != '\\' {
            continue;
        }

        let Some(&(next_idx, next_ch)) = chars.peek() else {
            continue;
        };
        let Some(command) = combining_latex_command(next_ch) else {
            continue;
        };

        let out = normalized.get_or_insert_with(|| String::with_capacity(input.len()));
        out.push_str(&input[last..idx]);
        out.push('\\');
        out.push_str(command);
        last = next_idx + next_ch.len_utf8();
        chars.next();
    }

    match normalized {
        Some(mut out) => {
            out.push_str(&input[last..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(input),
    }
}

fn normalize_display_text(text: &str) -> String {
    text.nfc().collect()
}

fn bib_field(entry: &biblatex::Entry, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        entry
            .get(key)
            .map(|chunks| normalize_display_text(chunks.format_verbatim().trim()))
            .filter(|value| !value.is_empty())
    })
}

fn insert_string_if_missing(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<String>,
) {
    if obj
        .get(key)
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
    {
        return;
    }
    if let Some(value) = value.filter(|s| !s.trim().is_empty()) {
        obj.insert(key.into(), json!(value));
    }
}

/// Read a free-form date (`urldate`) as CSL date-parts.
///
/// Year-first ISO ("2024-03-15", "2024/03/15", with or without a time) is
/// read as written. Day-first or month-first is read only when the other
/// order is impossible ("15/03/2024", "03/15/2024"); "05/03/2024" is not
/// guessed. Reading by position made the day the year — year 15. A month
/// outside 1–12 or a day outside 1–31 is left out, with what follows it.
fn date_parts_from_isoish(input: &str) -> Option<Value> {
    let date = input.trim().split(['T', ' ']).next()?;
    let mut numbers = Vec::new();
    for raw in date.split(['-', '/']).take(3) {
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) || raw.len() > 4 {
            break;
        }
        numbers.push((raw.len(), raw.parse::<i32>().ok()?));
    }
    let (year, month, day) = match numbers.as_slice() {
        [(4, y), rest @ ..] => (*y, rest.first().map(|m| m.1), rest.get(1).map(|d| d.1)),
        [(_, d), (_, m), (4, y)] if *d > 12 => (*y, Some(*m), Some(*d)),
        [(_, m), (_, d), (4, y)] if *d > 12 => (*y, Some(*m), Some(*d)),
        _ => return None,
    };
    let mut parts = vec![year];
    if let Some(month) = month.filter(|m| (1..=12).contains(m)) {
        parts.push(month);
        if let Some(day) = day.filter(|d| (1..=31).contains(d)) {
            parts.push(day);
        }
    }
    Some(json!({ "date-parts": [parts] }))
}

fn biblatex_type_to_csl(entry: &biblatex::Entry) -> Option<&'static str> {
    use biblatex::EntryType as T;

    match &entry.entry_type {
        T::Article => {
            if bib_field(entry, &["journaltitle", "journal"]).is_some() {
                Some("article-journal")
            } else {
                Some("article")
            }
        }
        T::Book | T::MvBook => Some("book"),
        T::InBook | T::InCollection | T::SuppBook | T::SuppCollection | T::InReference => {
            Some("chapter")
        }
        T::InProceedings | T::Proceedings | T::MvProceedings => Some("paper-conference"),
        T::MastersThesis | T::PhdThesis | T::Thesis => Some("thesis"),
        T::TechReport | T::Report => Some("report"),
        T::Online => Some("webpage"),
        T::Software => Some("software"),
        T::Dataset => Some("dataset"),
        T::Patent => Some("patent"),
        _ => None,
    }
}

/// Convert a Hayagriva person (parsed from a BibTeX name) to a CSL name.
///
/// The BibTeX "von" part becomes `non-dropping-particle`: BibTeX prints it
/// with the last name ("van der Berg", also in author-year labels), and
/// citation-js — CiteMe's JS path — maps it the same way. A name with no
/// given part is a `literal` (institutions: `{World Health Organization}`)
/// unless it carries a particle or suffix, which only a person has; folding
/// that into a literal dropped the "von" of "von Neumann".
fn person_to_csl(p: &hayagriva::types::Person) -> Value {
    let mut name = match &p.given_name {
        Some(given) => json!({
            "family": normalize_display_text(&p.name),
            "given": normalize_display_text(given),
        }),
        None if p.prefix.is_some() || p.suffix.is_some() => {
            json!({ "family": normalize_display_text(&p.name) })
        }
        None => return json!({ "literal": normalize_display_text(&p.name) }),
    };
    if let Some(prefix) = &p.prefix {
        name["non-dropping-particle"] = json!(normalize_display_text(prefix));
    }
    if let Some(suffix) = &p.suffix {
        name["suffix"] = json!(normalize_display_text(suffix));
    }
    name
}

/// Convert a Hayagriva Entry to a CSL-JSON serde_json::Value.
///
/// This is the Entry → CSL-JSON direction (used by parsers).
/// The formatting hot path uses CSL-JSON → BibliographyDriver directly.
fn entry_to_csl_json(entry: &Entry) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // Type mapping: Hayagriva EntryType → CSL-JSON type string
    let csl_type = match entry.entry_type() {
        hayagriva::types::EntryType::Article => {
            if entry
                .parents()
                .iter()
                .any(|p| *p.entry_type() == hayagriva::types::EntryType::Periodical)
            {
                "article-journal"
            } else if entry
                .parents()
                .iter()
                .any(|p| *p.entry_type() == hayagriva::types::EntryType::Proceedings)
            {
                "paper-conference"
            } else if entry
                .parents()
                .iter()
                .any(|p| *p.entry_type() == hayagriva::types::EntryType::Newspaper)
            {
                "article-newspaper"
            } else {
                "article"
            }
        }
        hayagriva::types::EntryType::Book => "book",
        hayagriva::types::EntryType::Chapter | hayagriva::types::EntryType::Anthos => "chapter",
        hayagriva::types::EntryType::Thesis => "thesis",
        hayagriva::types::EntryType::Report => "report",
        hayagriva::types::EntryType::Web | hayagriva::types::EntryType::Blog => "webpage",
        hayagriva::types::EntryType::Repository => "software",
        _ => "article",
    };
    obj.insert("type".into(), json!(csl_type));
    obj.insert("id".into(), json!(entry.key()));

    // Title
    if let Some(title) = entry.title() {
        let title = title.to_string();
        obj.insert("title".into(), json!(normalize_display_text(&title)));
    }

    // Authors / editors. BibTeX's `and others` truncates a name list (Google
    // Scholar writes it for long ones); read as a name, it became an author
    // literally called "others". CSL has no truncation marker, so drop it.
    let names = |people: &[hayagriva::types::Person]| -> Vec<Value> {
        people
            .iter()
            .filter(|p| {
                !(p.name.eq_ignore_ascii_case("others")
                    && p.given_name.is_none()
                    && p.prefix.is_none()
                    && p.suffix.is_none())
            })
            .map(person_to_csl)
            .collect()
    };
    if let Some(authors) = entry.authors() {
        let names = names(authors);
        if !names.is_empty() {
            obj.insert("author".into(), json!(names));
        }
    }
    if let Some(editors) = entry.editors() {
        let names = names(editors);
        if !names.is_empty() {
            obj.insert("editor".into(), json!(names));
        }
    }

    // Date. hayagriva's `Date` is zero-based (month 0-11, day 0-30); CSL
    // date-parts are one-based. Pushing them raw shifted every imported date
    // back — `month = may` became April and January became month 0.
    if let Some(date) = entry.date() {
        let mut parts: Vec<i32> = vec![date.year];
        if let Some(m) = date.month {
            parts.push(i32::from(m) + 1);
            if let Some(d) = date.day {
                parts.push(i32::from(d) + 1);
            }
        }
        obj.insert("issued".into(), json!({"date-parts": [parts]}));
    }

    // Container title (from first parent)
    if let Some(parent) = entry.parents().first() {
        if let Some(title) = parent.title() {
            let title = title.to_string();
            obj.insert(
                "container-title".into(),
                json!(normalize_display_text(&title)),
            );
        }
    }

    // Identifiers
    if let Some(doi) = entry.doi() {
        obj.insert("DOI".into(), json!(doi));
    }
    if let Some(isbn) = entry.isbn() {
        obj.insert("ISBN".into(), json!(isbn));
    }
    if let Some(issn) = entry.issn() {
        obj.insert("ISSN".into(), json!(issn));
    }
    if let Some(url) = entry.url_any() {
        obj.insert("URL".into(), json!(url.value.to_string()));
    }

    // Biblio fields
    if let Some(v) = entry.volume() {
        obj.insert("volume".into(), json!(v.to_string()));
    }
    if let Some(v) = entry.issue() {
        obj.insert("issue".into(), json!(v.to_string()));
    }
    if let Some(v) = entry.page_range() {
        obj.insert("page".into(), json!(v.to_string()));
    }
    if let Some(v) = entry.edition() {
        obj.insert("edition".into(), json!(v.to_string()));
    }

    // Abstract / note (FormatString::to_string strips markup)
    if let Some(s) = entry.abstract_() {
        let value = s.to_string();
        obj.insert("abstract".into(), json!(normalize_display_text(&value)));
    }
    if let Some(n) = entry.note() {
        let value = n.to_string();
        obj.insert("note".into(), json!(normalize_display_text(&value)));
    }

    // Chapter number
    if let Some(c) = entry.chapter() {
        obj.insert("chapter-number".into(), json!(c.to_string()));
    }

    // PMID / PMCID — CSL 1.0.1+ top-level variables
    if let Some(v) = entry.pmid() {
        obj.insert("PMID".into(), json!(v));
    }
    if let Some(v) = entry.pmcid() {
        obj.insert("PMCID".into(), json!(v));
    }

    // arXiv → custom.eprint (CSL has no top-level arxiv variable)
    if let Some(v) = entry.arxiv() {
        let custom = obj.entry("custom").or_insert_with(|| json!({}));
        if let Some(map) = custom.as_object_mut() {
            map.insert("eprint".into(), json!({ "id": v, "type": "arxiv" }));
        }
    }

    // Publisher
    if let Some(pub_) = entry.publisher() {
        if let Some(name) = pub_.name() {
            let value = name.to_string();
            obj.insert("publisher".into(), json!(normalize_display_text(&value)));
        }
        if let Some(loc) = pub_.location() {
            let value = loc.to_string();
            obj.insert(
                "publisher-place".into(),
                json!(normalize_display_text(&value)),
            );
        }
    }

    // Genre. biblatex's `type` holds localization keys (`phdthesis`,
    // `mathesis`, …) that hayagriva copies verbatim; spelled as a genre they
    // printed "[Phdthesis]". Known keys become the text biblatex would print.
    if let Some(genre) = entry.genre() {
        let value = genre.to_string();
        let value = match value.trim() {
            "phdthesis" => "Doctoral dissertation".to_string(),
            "mathesis" => "Master's thesis".to_string(),
            "candthesis" => "Candidate thesis".to_string(),
            "techreport" => "technical report".to_string(),
            "resreport" => "research report".to_string(),
            _ => value,
        };
        obj.insert("genre".into(), json!(normalize_display_text(&value)));
    }

    // Language
    if let Some(lang) = entry.language() {
        let value = lang.to_string();
        obj.insert("language".into(), json!(normalize_display_text(&value)));
    }

    serde_json::Value::Object(obj)
}

/// Fill in what hayagriva's BibLaTeX interop drops or gets wrong, read from
/// the same `biblatex` entry the CSL entry was converted from: the entry
/// type, `keywords`, `series`, standalone `pmid`/`pmcid`,
/// `eprint`/`eprinttype`/`eprintclass`, the start of a date range, a
/// numeric month, a page list, a URL given in `howpublished`/`note`, and
/// the key as written.
///
/// This used to merge a second `biblatex` parse into hayagriva's entries by
/// cite key and occurrence count. The count shifted whenever hayagriva
/// rejected one of two entries sharing a key, and the survivor took its
/// twin's type and journal.
fn apply_biblatex_fields(csl: &mut Value, bib_entry: &biblatex::Entry, notes: Option<&EntryNotes>) {
    let Some(obj) = csl.as_object_mut() else {
        return;
    };

    // A duplicate key parses under a fresh one; the id is the key written.
    if let Some(notes) = notes {
        obj.insert("id".into(), json!(notes.key));
    }

    if let Some(csl_type) = biblatex_type_to_csl(bib_entry) {
        obj.insert("type".into(), json!(csl_type));
    }

    insert_string_if_missing(
        obj,
        "container-title",
        bib_field(
            bib_entry,
            &["journaltitle", "journal", "booktitle", "eventtitle"],
        ),
    );
    insert_string_if_missing(obj, "event-title", bib_field(bib_entry, &["eventtitle"]));
    insert_string_if_missing(
        obj,
        "publisher-place",
        bib_field(bib_entry, &["location", "address"]),
    );
    insert_string_if_missing(
        obj,
        "publisher",
        bib_field(bib_entry, &["publisher", "institution", "school"]),
    );
    insert_string_if_missing(obj, "volume", bib_field(bib_entry, &["volume"]));
    insert_string_if_missing(obj, "issue", bib_field(bib_entry, &["number", "issue"]));
    insert_string_if_missing(obj, "page", bib_field(bib_entry, &["pages"]));
    insert_string_if_missing(obj, "URL", bib_field(bib_entry, &["url"]));
    insert_string_if_missing(obj, "DOI", bib_field(bib_entry, &["doi"]));
    insert_string_if_missing(obj, "version", bib_field(bib_entry, &["version"]));

    if !obj.contains_key("accessed") {
        if let Some(date) = bib_field(bib_entry, &["urldate"]) {
            if let Some(date_parts) = date_parts_from_isoish(&date) {
                obj.insert("accessed".into(), date_parts);
            }
        }
    }

    // keywords: CSL uses `keyword` (singular, comma-separated string per v1.0 schema)
    if !obj.contains_key("keyword") {
        if let Ok(chunks) = bib_entry.keywords() {
            let raw = chunks.format_verbatim();
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                obj.insert("keyword".into(), json!(normalize_display_text(trimmed)));
            }
        }
    }

    // series → collection-title. Hayagriva nests series under a synthetic
    // parent whose type inherits from the main parent (e.g. Anthology for
    // incollection), making it awkward to traverse. Reading the raw field
    // is simpler and matches what BibTeX users expect.
    if !obj.contains_key("collection-title") {
        if let Ok(chunks) = bib_entry.series() {
            let raw = chunks.format_verbatim();
            let v = normalize_display_text(raw.trim());
            if !v.is_empty() {
                obj.insert("collection-title".into(), json!(v));
            }
        }
    }

    // Direct `pmid`/`pmcid` fields: hayagriva 0.9 only reads these from
    // `eprint`/`eprinttype = pubmed`; a standalone `pmid = {...}` is dropped.
    // Fill from biblatex Entry::get() when not already present.
    if !obj.contains_key("PMID") {
        if let Some(chunks) = bib_entry.get("pmid") {
            let v = chunks.format_verbatim().trim().to_string();
            if !v.is_empty() {
                obj.insert("PMID".into(), json!(v));
            }
        }
    }
    if !obj.contains_key("PMCID") {
        if let Some(chunks) = bib_entry.get("pmcid") {
            let v = chunks.format_verbatim().trim().to_string();
            if !v.is_empty() {
                obj.insert("PMCID".into(), json!(v));
            }
        }
    }

    // eprint / eprinttype / eprintclass → custom.eprint = { id, type, class }
    //
    // Require both id AND type to emit. An `eprint = {…}` without
    // `eprinttype` is meaningless to downstream styles (they need the type
    // to route to arxiv, pubmed, etc.); emitting a type-less `custom.eprint`
    // would leak into our BibTeX exporter as a broken `eprint = {…}`
    // without `eprinttype = {…}` that many style files choke on.
    //
    // If hayagriva already set `custom.eprint` (via `entry.arxiv()` when
    // `eprinttype = arxiv`), augment with `class` but preserve id/type.
    if let Ok(id) = bib_entry.eprint() {
        let id = id.trim().to_string();
        let eprint_type = bib_entry
            .eprint_type()
            .ok()
            .map(|c| c.format_verbatim().trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let eprint_class = bib_entry
            .eprint_class()
            .ok()
            .map(|c| c.format_verbatim().trim().to_string())
            .filter(|s| !s.is_empty());

        // Require id + type to avoid leaking incomplete `custom.eprint`
        // into downstream exports. If hayagriva already populated
        // `custom.eprint` via `entry.arxiv()`, this branch also fires
        // (biblatex reads the same `eprinttype` field), and we layer
        // `class` in via the map-merge below without clobbering id/type.
        if !id.is_empty() && eprint_type.is_some() {
            let custom = obj.entry("custom").or_insert_with(|| json!({}));
            if let Some(map) = custom.as_object_mut() {
                let mut eprint_obj = match map.get("eprint").cloned() {
                    Some(Value::Object(m)) => m,
                    _ => serde_json::Map::new(),
                };
                eprint_obj.entry("id").or_insert_with(|| json!(id));
                if let Some(t) = eprint_type {
                    eprint_obj.entry("type").or_insert_with(|| json!(t));
                }
                if let Some(c) = eprint_class {
                    eprint_obj.insert("class".into(), json!(c));
                }
                if !eprint_obj.is_empty() {
                    map.insert("eprint".into(), Value::Object(eprint_obj));
                }
            }
        }
    }

    // hayagriva reads a biblatex range as its end: `date = {2020/2021}`
    // imported as 2021. A single CSL date is where the range starts.
    if let Ok(PermissiveType::Typed(date)) = bib_entry.date() {
        if let DateValue::Between(start, _) = date.value {
            let mut parts = vec![start.year];
            if let Some(month) = start.month {
                parts.push(i32::from(month) + 1);
                if let Some(day) = start.day {
                    parts.push(i32::from(day) + 1);
                }
            }
            obj.insert("issued".into(), json!({ "date-parts": [parts] }));
        }
    }

    // biblatex 0.11 reads `month` only as a name, so `month = {5}` — the form
    // biblatex documents — was dropped, and a `day` with it.
    if bib_entry.get("date").is_none() {
        let number = |key: &str, max: i32| {
            bib_entry
                .get(key)
                .map(|chunks| chunks.format_verbatim())
                .and_then(|text| text.trim().parse::<i32>().ok())
                .filter(|n| (1..=max).contains(n))
        };
        let year_only = obj
            .get_mut("issued")
            .and_then(|issued| issued["date-parts"][0].as_array_mut())
            .filter(|parts| parts.len() == 1);
        if let (Some(parts), Some(month)) = (year_only, number("month", 12)) {
            parts.push(json!(month));
            if let Some(day) = number("day", 31) {
                parts.push(json!(day));
            }
        }
    }

    // A page list ("100--115, 200") came out of hayagriva without its
    // separators: "100-115200".
    if let Some(pages) = bib_entry
        .get("pages")
        .map(|chunks| chunks.format_verbatim())
    {
        if pages.contains(',') {
            let pages = pages.trim().replace(['–', '—'], "-");
            obj.insert("page".into(), json!(normalize_display_text(&pages)));
        }
    }

    if !obj.contains_key("URL") {
        if let Some(url) = notes.and_then(|notes| notes.url.as_deref()) {
            obj.insert("URL".into(), json!(url));
        }
    }
}

/// Convert one `biblatex` entry, dropping a field hayagriva rejects rather
/// than the entry. `year = {in press}` failed the date conversion and took
/// the whole entry with it — and, while the file was converted in one go,
/// every other entry too.
fn convert_entry(
    bib_entry: &biblatex::Entry,
) -> Result<(Entry, Cow<'_, biblatex::Entry>), TypeError> {
    let mut current = Cow::Borrowed(bib_entry);
    loop {
        let error = match Entry::try_from(current.as_ref()) {
            Ok(entry) => return Ok((entry, current)),
            Err(error) => error,
        };
        let Some(field) = field_to_drop(&current, &error) else {
            return Err(error);
        };
        current.to_mut().remove(&field);
    }
}

/// A `year` or `date` written as words — "in press", "forthcoming" — is a
/// publication status, CSL's `status`, which styles print where the date
/// would be. Dropped as an unreadable date, it rendered "(n.d.)".
fn status_from_dropped_date(
    csl: &mut Value,
    written: &biblatex::Entry,
    converted: &biblatex::Entry,
) {
    let Some(obj) = csl.as_object_mut() else {
        return;
    };
    for key in ["year", "date"] {
        if converted.get(key).is_some() {
            continue;
        }
        let Some(text) = written.get(key).map(|chunks| chunks.format_verbatim()) else {
            continue;
        };
        let text = text.trim();
        if !text.is_empty() && text.chars().all(|c| c.is_alphabetic() || c == ' ') {
            obj.entry("status")
                .or_insert_with(|| json!(normalize_display_text(text)));
            return;
        }
    }
}

/// The date fields hayagriva converts, least significant part first.
const DATE_FIELDS: &[&str] = &[
    "day",
    "month",
    "year",
    "date",
    "urlday",
    "urlmonth",
    "urlyear",
    "urldate",
    "eventday",
    "eventmonth",
    "eventyear",
    "eventdate",
    "origday",
    "origmonth",
    "origyear",
    "origdate",
];

/// The field a conversion error is about: the one its span falls in — but
/// biblatex reports some date errors (a day read out of `month`) with a span
/// relative to the value, so a date error outside the date fields falls
/// back to them, least significant first.
fn field_to_drop(entry: &biblatex::Entry, error: &TypeError) -> Option<String> {
    let date_only = matches!(
        error.kind,
        TypeErrorKind::UndefinedRange
            | TypeErrorKind::DayOutOfRange
            | TypeErrorKind::MonthOutOfRange
            | TypeErrorKind::MissingNumber
            | TypeErrorKind::WrongNumberOfDigits
            | TypeErrorKind::YearZeroCE
    );
    let at = error.span.start;
    let by_span = entry
        .fields
        .iter()
        .filter(|(_, chunks)| {
            let span = chunks.span();
            span.start < span.end && span.contains(&at)
        })
        .min_by_key(|(_, chunks)| chunks.span().len())
        .map(|(key, _)| key.as_str())
        .filter(|key| DATE_FIELDS.contains(key) || !date_only);
    by_span
        .or_else(|| {
            DATE_FIELDS
                .iter()
                .copied()
                .find(|key| entry.fields.contains_key(*key))
        })
        .map(str::to_string)
}

/// Parse BibTeX/BibLaTeX content into CSL-JSON.
pub fn parse_bibtex(input: &str, options: &ParseOptions) -> ParseResult {
    if input.trim().is_empty() {
        return ParseResult {
            entries: vec![],
            errors: vec![],
            format: "bibtex".to_string(),
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
            format: "bibtex".to_string(),
            truncated: true,
            scanned_entries: 0,
        };
    }

    let normalized_input = normalize_bibtex_input(input);
    let guarded = bibtex_guard::parse(normalized_input.as_ref());
    let mut errors = guarded.errors;
    let mut entries = Vec::new();
    let mut truncated = false;

    // One parse for the whole file, then one conversion per entry: an entry
    // hayagriva can't convert costs that entry, not the file.
    for bib_entry in &guarded.parsed {
        if options.max_entries.is_some_and(|max| entries.len() >= max) {
            truncated = true;
            break;
        }
        let notes = guarded.notes.get(&bib_entry.key);
        match convert_entry(bib_entry) {
            Ok((entry, converted)) => {
                let mut csl = entry_to_csl_json(&entry);
                apply_biblatex_fields(&mut csl, &converted, notes);
                status_from_dropped_date(&mut csl, bib_entry, &converted);
                entries.push(csl);
            }
            Err(error) => {
                if errors.len() < MAX_PARSE_ERRORS {
                    errors.push(ParseErrorInfo {
                        preview: notes.map(|n| n.preview.clone()).unwrap_or_default(),
                        error: format!("biblatex type error: {error}"),
                    });
                }
            }
        }
    }

    ParseResult {
        entries,
        errors,
        format: "bibtex".to_string(),
        truncated,
        scanned_entries: guarded.scanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bibtex_basic() {
        let input = include_str!("../../../../tests/fixtures/samples/sample.bib");
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(result.entries.len(), 2, "should parse 2 entries");
        assert!(
            result.errors.is_empty(),
            "should have no errors: {:?}",
            result.errors
        );
        assert!(!result.truncated);

        let first = &result.entries[0];
        let title = first.get("title").and_then(|v| v.as_str());
        assert!(title.is_some(), "first entry should have a title");
    }

    #[test]
    fn test_parse_bibtex_empty() {
        let result = parse_bibtex("", &ParseOptions::default());
        assert!(result.entries.is_empty());
    }

    #[test]
    fn test_parse_bibtex_malformed() {
        let input = "@article{bad, title = {unclosed brace";
        let result = parse_bibtex(input, &ParseOptions::default());
        assert!(
            !result.entries.is_empty() || !result.errors.is_empty(),
            "malformed input should produce either partial entries or errors, not empty result"
        );
    }

    #[test]
    fn test_parse_bibtex_preserves_metadata() {
        let input = include_str!("../../../../tests/fixtures/samples/metadata-rich.bib");
        let result = parse_bibtex(input, &ParseOptions::default());
        assert_eq!(
            result.entries.len(),
            3,
            "should parse 3 entries: {:?}",
            result.errors
        );
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );

        let first = &result.entries[0];
        assert_eq!(first["abstract"], "This paper studies Z.");
        assert_eq!(first["note"], "Funded by NSF");
        // CSL `keyword` is singular, string (per CSL v1.0 schema). Only the
        // biblatex-merge path populates this (hayagriva doesn't expose keywords).
        let kw = first["keyword"].as_str().expect("keyword must be a string");
        assert!(
            kw.contains("machine learning") && kw.contains("NLP"),
            "keyword should carry both tags: {kw}"
        );
        // PMID/PMCID top-level. hayagriva only reads PMID from `eprinttype=pubmed`;
        // this fixture has `eprinttype=arxiv`, so PMID/PMCID are load-bearing on
        // the merge path specifically.
        assert_eq!(first["PMID"], "38123456");
        assert_eq!(first["PMCID"], "PMC9876543");
        // custom.eprint populated either by hayagriva.arxiv() (id+type) or the
        // merge (id+type+class). `class` is the merge-only signal.
        assert_eq!(first["custom"]["eprint"]["id"], "2301.12345");
        assert_eq!(first["custom"]["eprint"]["type"], "arxiv");
        assert_eq!(
            first["custom"]["eprint"]["class"], "cs.CL",
            "eprintclass is only reachable via the biblatex merge path: {}",
            first["custom"]
        );

        let second = &result.entries[1];
        assert_eq!(second["chapter-number"], "7");
        assert_eq!(
            second["collection-title"], "Studies in Knowledge",
            "series should map to collection-title: {second}"
        );

        // Legacy `journal = {…}` (classic BibTeX) still parses as article with
        // container-title and keywords picked up by the merge.
        let third = &result.entries[2];
        assert_eq!(third["type"], "article-journal");
        assert_eq!(third["container-title"], "Old Journal");
        assert_eq!(third["keyword"], "legacy");
    }

    #[test]
    fn test_parse_biblatex_article_fields() {
        let input = r#"@article{smith2023,
  author = {Smith, Jane},
  title = {Deep Learning},
  journaltitle = {Nature},
  date = {2023-05-17},
  volume = {617},
  number = {7960},
  pages = {100--115},
  doi = {10.1038/s41586-023-12345},
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "expected one entry: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["type"], "article-journal");
        assert_eq!(first["title"], "Deep Learning");
        assert_eq!(first["container-title"], "Nature");
        assert_eq!(first["issued"]["date-parts"][0][0], 2023);
        assert_eq!(first["volume"], "617");
        assert_eq!(first["issue"], "7960");
        assert_eq!(first["page"], "100-115");
        assert_eq!(first["DOI"], "10.1038/s41586-023-12345");
    }

    #[test]
    fn test_parse_biblatex_conference_event_and_location() {
        let input = r#"@inproceedings{foo2022,
  author = {Foo, A.},
  title = {Talk Title},
  eventtitle = {NeurIPS 2022},
  booktitle = {Proceedings of NeurIPS},
  date = {2022-12},
  location = {New Orleans},
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "expected one entry: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["type"], "paper-conference");
        assert_eq!(first["container-title"], "Proceedings of NeurIPS");
        assert_eq!(first["event-title"], "NeurIPS 2022");
        assert_eq!(first["publisher-place"], "New Orleans");
    }

    #[test]
    fn test_parse_biblatex_dataset_and_software_types() {
        let input = r#"@dataset{data2023,
  author = {Lab, Some},
  title = {Dataset X},
  publisher = {Zenodo},
  date = {2023},
  doi = {10.5281/zenodo.1234567},
}

@software{tool2024,
  author = {Dev Team},
  title = {A Tool},
  version = {1.2.0},
  date = {2024-03-01},
  url = {https://github.com/x/y},
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            2,
            "expected two entries: {:?}",
            result.errors
        );
        let dataset = &result.entries[0];
        assert_eq!(dataset["type"], "dataset");
        assert_eq!(dataset["DOI"], "10.5281/zenodo.1234567");
        assert_eq!(dataset["publisher"], "Zenodo");

        let software = &result.entries[1];
        assert_eq!(software["type"], "software");
        assert_eq!(software["title"], "A Tool");
        assert_eq!(software["URL"], "https://github.com/x/y");
        assert_eq!(software["version"], "1.2.0");
    }

    #[test]
    fn test_parse_bibtex_duplicate_keys_do_not_break_merge() {
        // biblatex::Bibliography::parse returns DuplicateKey and bails, while
        // hayagriva tolerates duplicates. Without the chunk-level fallback in
        // collect_biblatex_entries, the presence of any duplicate would silently
        // wipe out keywords/series/pmid/eprintclass for every other entry.
        let input = r#"@article{smith2024,
  author = {Smith},
  title = {A},
  journal = {X},
  year = {2024},
  keywords = {ml}
}

@article{smith2024,
  author = {Smith},
  title = {B},
  journal = {Y},
  year = {2024}
}

@article{doe2023,
  author = {Doe},
  title = {C},
  journal = {Z},
  year = {2023},
  keywords = {nlp}
}
"#;
        let result = parse_bibtex(input, &ParseOptions::default());
        // hayagriva keeps both smith entries + doe = 3.
        assert_eq!(
            result.entries.len(),
            3,
            "hayagriva tolerates dupes: {:?}",
            result.errors
        );
        // The non-duplicated `doe2023` must still have its keyword merged even
        // though `smith2024` is duplicated earlier in the file.
        let doe = result
            .entries
            .iter()
            .find(|e| e["id"] == "doe2023")
            .expect("doe2023 present");
        assert_eq!(
            doe["keyword"], "nlp",
            "merge must salvage keywords on entries with unique keys even when \
             the file contains duplicate-key entries elsewhere"
        );
    }

    #[test]
    fn test_parse_bibtex_duplicate_keys_merge_with_their_own_entry() {
        // Every occurrence of a duplicated key used to be merged with the
        // FIRST biblatex entry of that key: the second `dup` below — a book —
        // came out as article-journal, with the first entry's journal and
        // keywords.
        let input = r#"@article{dup,
  author = {Smith, A},
  title = {A},
  journal = {X},
  year = {2024},
  keywords = {ml}
}

@book{dup,
  author = {Doe, B},
  title = {B},
  publisher = {P},
  year = {2020},
  keywords = {history}
}
"#;
        let result = parse_bibtex(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 2, "{:?}", result.errors);
        let (first, second) = (&result.entries[0], &result.entries[1]);
        assert_eq!(first["type"], "article-journal");
        assert_eq!(first["keyword"], "ml");
        assert_eq!(second["title"], "B");
        assert_eq!(second["type"], "book", "{second}");
        assert_eq!(second["keyword"], "history", "{second}");
        assert!(second.get("container-title").is_none(), "{second}");
    }

    #[test]
    fn test_parse_bibtex_von_part_is_non_dropping() {
        // BibTeX prints the von part with the last name ("van der Berg"),
        // which is CSL's non-dropping particle; citation-js maps it the same
        // way. As a dropping particle, APA rendered "(Berg, 2000)".
        let input = "@book{v, author = {van der Berg, Jan and Ludwig van Beethoven and von Neumann and {World Health Organization}}, editor = {de la Fontaine, Jean}, title = {T}, year = {2000}}";
        let result = parse_bibtex(input, &ParseOptions::default());
        let first = &result.entries[0];
        assert_eq!(
            first["author"][0],
            json!({"family": "Berg", "given": "Jan", "non-dropping-particle": "van der"})
        );
        assert_eq!(first["author"][1]["non-dropping-particle"], "van");
        // No given name: the particle used to vanish into `literal: "Neumann"`.
        assert_eq!(
            first["author"][2],
            json!({"family": "Neumann", "non-dropping-particle": "von"})
        );
        assert_eq!(
            first["author"][3],
            json!({"literal": "World Health Organization"})
        );
        assert_eq!(first["editor"][0]["non-dropping-particle"], "de la");
    }

    #[test]
    fn test_parse_bibtex_month_and_day_are_one_based() {
        // hayagriva's Date stores month 0-11 and day 0-30; pushing them raw
        // shifted every date back: `month = may` became April, and January
        // became month 0 (APA rendered "(2019, 256)").
        let input = "@article{a, title={A}, year={2019}, month=may}\n\n@article{b, title={B}, date={2019-01-01}}\n\n@article{c, title={C}, date={2020-12-31}}";
        let result = parse_bibtex(input, &ParseOptions::default());
        let dates: Vec<&Value> = result
            .entries
            .iter()
            .map(|e| &e["issued"]["date-parts"][0])
            .collect();
        assert_eq!(dates[0], &json!([2019, 5]));
        assert_eq!(dates[1], &json!([2019, 1, 1]));
        assert_eq!(dates[2], &json!([2020, 12, 31]));
    }

    #[test]
    fn test_parse_biblatex_thesis_type_keys_become_readable_genres() {
        // biblatex files (JabRef, our own exporter before) carry localization
        // keys in `type`; hayagriva copies them verbatim into genre, so APA
        // printed "[Phdthesis]".
        let input = "@thesis{a, author={A, B}, title={T}, type={phdthesis}, institution={U}, date={2020}}\n\n@thesis{b, author={C, D}, title={T2}, type={mathesis}, institution={U}, date={2020}}\n\n@thesis{c, author={E, F}, title={T3}, type={Tese (Doutorado em Letras)}, institution={U}, date={2020}}";
        let result = parse_bibtex(input, &ParseOptions::default());
        assert_eq!(result.entries[0]["genre"], "Doctoral dissertation");
        assert_eq!(result.entries[1]["genre"], "Master's thesis");
        assert_eq!(result.entries[2]["genre"], "Tese (Doutorado em Letras)");
    }

    #[test]
    fn test_parse_bibtex_eprint_without_type_is_dropped() {
        // An `eprint = {…}` with no `eprinttype` is ambiguous (arxiv? pubmed?
        // hdl?) — emitting custom.eprint with only `id` would leak into exports
        // as broken fields. The parser must drop it.
        let input = r#"@article{x,
  author = {X},
  title = {T},
  journal = {J},
  year = {2024},
  eprint = {something.12345}
}"#;
        let result = parse_bibtex(input, &ParseOptions::default());
        assert_eq!(result.entries.len(), 1);
        let first = &result.entries[0];
        assert!(
            first["custom"]["eprint"].is_null(),
            "eprint without eprinttype must not populate custom.eprint: {}",
            first["custom"]
        );
    }

    #[test]
    fn test_parse_bibtex_normalizes_combining_latex_commands() {
        let input = "@article{x,\n  title = {Aspectos jur\\\u{301}idicos e Jo\\\u{303}ao},\n  author = {Silva, Jo\\\u{303}ao and Garc\\\u{301}ia, Mar\\\u{301}ia},\n  journal = {Revista},\n  year = {2024}\n}";
        let result = parse_bibtex(input, &ParseOptions::default());

        assert_eq!(
            result.entries.len(),
            1,
            "expected one parsed entry: {:?}",
            result.errors
        );
        let first = &result.entries[0];
        assert_eq!(first["title"], "Aspectos jurídicos e João");
        assert_eq!(first["author"][0]["family"], "Silva");
        assert_eq!(first["author"][0]["given"], "João");
        assert_eq!(first["author"][1]["family"], "García");
        assert_eq!(first["author"][1]["given"], "María");
    }
}
