pub mod biblatex;
pub mod bibtex;
pub mod hayagriva;
pub mod ris;

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
/// when in 1..=12 and `day` only when in 1..=31 with a valid month, so no
/// exporter can emit an impossible date like `2024-13`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DateParts {
    pub year: i64,
    pub month: Option<i64>,
    pub day: Option<i64>,
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
        None => date["raw"]
            .as_str()?
            .trim()
            .split(['-', '/'])
            .map(|p| p.trim().parse().ok())
            .collect(),
    };
    let year = (*numbers.first()?)?;
    let month = numbers
        .get(1)
        .copied()
        .flatten()
        .filter(|m| (1..=12).contains(m));
    let day = month.and_then(|_| {
        numbers
            .get(2)
            .copied()
            .flatten()
            .filter(|d| (1..=31).contains(d))
    });
    Some(DateParts { year, month, day })
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
    fn text_field_accepts_strings_and_numbers() {
        let item = json!({"volume": 42, "issue": "3", "page": null});
        assert_eq!(text_field(&item, "volume").as_deref(), Some("42"));
        assert_eq!(text_field(&item, "issue").as_deref(), Some("3"));
        assert_eq!(text_field(&item, "page"), None);
        assert_eq!(text_field(&item, "missing"), None);
    }
}
