pub mod biblatex;
pub mod bibtex;
pub mod hayagriva;
pub mod ris;

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// A CSL "string or number" variable as text.
///
/// CSL-JSON allows numbers for `volume`, `issue`, `page`, `edition`,
/// `chapter-number`, … (`"volume": 42`). Reading them with `as_str()`
/// silently dropped them from every exporter.
pub(crate) fn text_field(item: &Value, key: &str) -> Option<String> {
    match &item[key] {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A CSL date reduced to what the exporters can write. `month` is only set
/// when in 1..=12 and `day` only when that month has it, so no exporter can
/// emit an impossible date like `2024-13` or `2019-02-30` (hayagriva refuses
/// the whole file over the latter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DateParts {
    pub year: i64,
    pub month: Option<i64>,
    pub day: Option<i64>,
}

impl DateParts {
    /// The year as ISO 8601 / EDTF writes it: four digits, signed when
    /// negative (`0050`, `-0350`). `None` past four digits, which neither
    /// biblatex nor hayagriva reads back in a full date.
    pub(crate) fn iso_year(&self) -> Option<String> {
        let year = self.year;
        if !(-9999..=9999).contains(&year) {
            return None;
        }
        Some(if year < 0 {
            format!("-{:04}", year.unsigned_abs())
        } else {
            format!("{year:04}")
        })
    }

    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, or `None` when the year has no
    /// four-digit form (see `iso_year`).
    pub(crate) fn iso(&self) -> Option<String> {
        let year = self.iso_year()?;
        Some(match (self.month, self.day) {
            (Some(m), Some(d)) => format!("{year}-{m:02}-{d:02}"),
            (Some(m), None) => format!("{year}-{m:02}"),
            _ => year,
        })
    }
}

/// Days in a month of the proleptic Gregorian calendar (`year` astronomical,
/// as CSL and hayagriva count it).
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Read the first date of a CSL date variable (`issued`, `accessed`, …).
///
/// Each date-part may be a number or a numeric string — `[["2019", "5"]]`
/// is valid CSL-JSON, and what Zotero and citation-js emit — and a `raw`
/// ISO-like date ("2018-07") is read when there are no date-parts. Reading
/// only `as_i64()` dropped the year from every export.
pub(crate) fn date_parts(item: &Value, key: &str) -> Option<DateParts> {
    let date = &item[key];
    let numbers: Vec<Option<i64>> = match date["date-parts"].get(0).and_then(Value::as_array) {
        Some(parts) => parts.iter().map(date_part_number).collect(),
        None => raw_iso_parts(date["raw"].as_str()?)?,
    };
    let year = (*numbers.first()?)?;
    let month = numbers
        .get(1)
        .copied()
        .flatten()
        .filter(|m| (1..=12).contains(m));
    let day = month.and_then(|m| {
        numbers
            .get(2)
            .copied()
            .flatten()
            .filter(|d| (1..=days_in_month(year, m)).contains(d))
    });
    Some(DateParts { year, month, day })
}

/// Make keys unique, in order. The first item with a key keeps it; each
/// later duplicate takes the next `candidate(base, n)` (n = 1, 2, …) that is
/// free. A candidate is never a key another item has as its own — exporting
/// `["x", "x", "x-2"]` must not hand the explicit `x-2` to the second `x` —
/// and the counter is kept per base, so a run of duplicates costs linear
/// time, not a fresh probe from `n = 1` for each one. `fold` decides when
/// two keys are the same (BibTeX compares them without regard to case).
pub(crate) fn unique_keys(
    bases: Vec<String>,
    candidate: impl Fn(&str, usize) -> String,
    fold: impl Fn(&str) -> String,
) -> Vec<String> {
    let own: HashSet<String> = bases.iter().map(|b| fold(b)).collect();
    let mut taken: HashSet<String> = HashSet::with_capacity(bases.len());
    let mut next: HashMap<String, usize> = HashMap::new();
    bases
        .into_iter()
        .map(|base| {
            let folded = fold(&base);
            if taken.insert(folded.clone()) {
                return base;
            }
            let n = next.entry(folded).or_insert(0);
            loop {
                *n += 1;
                let key = candidate(&base, *n);
                let folded = fold(&key);
                if !own.contains(&folded) && taken.insert(folded) {
                    return key;
                }
            }
        })
        .collect()
}

/// A CSL date variable as an ISO 8601 date (`YYYY[-MM[-DD]]`), the form
/// biblatex `date`/`urldate` and hayagriva read. `None` when absent or when
/// the year has no four-digit form.
pub(crate) fn iso_date(item: &Value, key: &str) -> Option<String> {
    date_parts(item, key)?.iso()
}

/// `raw` is free text; only a year-first ISO form ("2018", "2018-07",
/// "2018-07-15T10:00:00Z") is read. "05/03/2019" is day/month or month/day,
/// and reading it by position made the year 5.
pub(crate) fn raw_iso_parts(raw: &str) -> Option<Vec<Option<i64>>> {
    let date = raw.trim().split(['T', ' ']).next()?;
    let mut parts = date.split('-');
    let year = parts.next()?;
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(
        std::iter::once(year)
            .chain(parts)
            .map(|p| p.parse().ok())
            .collect(),
    )
}

fn date_part_number(part: &Value) -> Option<i64> {
    match part {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn date_parts_accept_numbers_strings_and_raw() {
        let parts = |v: Value| date_parts(&json!({ "issued": v }), "issued");
        let full = Some(DateParts {
            year: 2019,
            month: Some(5),
            day: Some(3),
        });
        assert_eq!(parts(json!({"date-parts": [[2019, 5, 3]]})), full);
        assert_eq!(parts(json!({"date-parts": [["2019", "5", "3"]]})), full);
        assert_eq!(parts(json!({"raw": "2019-05-03"})), full);
        assert_eq!(
            parts(json!({"date-parts": [[2024, 13, 40]]})),
            Some(DateParts {
                year: 2024,
                month: None,
                day: None
            })
        );
        assert_eq!(parts(json!({"literal": "Spring 2020"})), None);
        assert_eq!(parts(json!({"date-parts": [["n.d."]]})), None);
    }

    #[test]
    fn date_parts_drop_a_day_its_month_does_not_have() {
        let parts = |v: Value| date_parts(&json!({ "issued": {"date-parts": v} }), "issued");
        let day = |v: Value| parts(v).and_then(|d| d.day);
        assert_eq!(day(json!([[2019, 2, 30]])), None);
        assert_eq!(day(json!([[2019, 4, 31]])), None);
        assert_eq!(day(json!([[2019, 2, 29]])), None);
        assert_eq!(day(json!([[2020, 2, 29]])), Some(29));
        assert_eq!(day(json!([[2000, 2, 29]])), Some(29));
        assert_eq!(day(json!([[1900, 2, 29]])), None);
        assert_eq!(day(json!([[2019, 12, 31]])), Some(31));
        assert_eq!(parts(json!([[2019, 2, 30]])).and_then(|d| d.month), Some(2));
    }

    #[test]
    fn iso_years_are_four_digits_and_signed() {
        let iso = |v: Value| {
            date_parts(&json!({ "issued": {"date-parts": v} }), "issued").and_then(|d| d.iso())
        };
        assert_eq!(iso(json!([[2024, 3, 5]])).as_deref(), Some("2024-03-05"));
        assert_eq!(iso(json!([[50]])).as_deref(), Some("0050"));
        assert_eq!(iso(json!([[800, 3]])).as_deref(), Some("0800-03"));
        assert_eq!(iso(json!([[-350]])).as_deref(), Some("-0350"));
        assert_eq!(iso(json!([[0]])).as_deref(), Some("0000"));
        assert_eq!(iso(json!([[20240]])), None);
        assert_eq!(iso(json!([[-10000]])), None);
    }

    #[test]
    fn unique_keys_keep_first_holders_and_never_take_an_own_key() {
        let keys = |bases: &[&str]| {
            unique_keys(
                bases.iter().map(|b| b.to_string()).collect(),
                |base, n| format!("{base}-{n}"),
                str::to_string,
            )
        };
        assert_eq!(keys(&["a", "b", "a", "a"]), ["a", "b", "a-1", "a-2"]);
        // The second `x` must not take `x-1`, which a later item owns.
        assert_eq!(keys(&["x", "x", "x-1"]), ["x", "x-2", "x-1"]);
        // Folding decides sameness; output keeps each base's own spelling.
        let folded = unique_keys(
            vec!["K".into(), "k".into()],
            |base, n| format!("{base}{n}"),
            str::to_lowercase,
        );
        assert_eq!(folded, ["K", "k1"]);
    }

    #[test]
    fn text_field_accepts_strings_and_numbers() {
        let item = json!({"volume": 42, "issue": "3", "page": null});
        assert_eq!(text_field(&item, "volume").as_deref(), Some("42"));
        assert_eq!(text_field(&item, "issue").as_deref(), Some("3"));
        assert_eq!(text_field(&item, "page"), None);
        assert_eq!(text_field(&item, "missing"), None);
    }
}
