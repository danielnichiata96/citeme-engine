//! CSL-JSON items in the shape hayagriva can read.
//!
//! hayagriva deserializes each item into `citationberg::json::Item`, a map
//! whose values must each be a string, an integer, a list of name objects or
//! a date object. Anything else fails the whole item even though hayagriva
//! would never read it: a `null`, a boolean, a float, a keyword list, an
//! extension object such as `custom` (which CiteMe puts on every arXiv
//! paper). Two date shapes get further and panic in the renderer, aborting
//! the Wasm instance: an empty date (`"date-parts": [[]]` — citationberg
//! unwraps its first number) and any range ("ranges are not supported").
//!
//! [`prepare`] rewrites an item into what hayagriva reads. It drops only
//! what hayagriva would ignore anyway, and refuses the rest with a reason.

use serde_json::{Map, Value};

/// The name variables citationberg reads.
const NAME_VARIABLES: &[&str] = &[
    "author",
    "chair",
    "collection-editor",
    "compiler",
    "composer",
    "container-author",
    "contributor",
    "curator",
    "director",
    "editor",
    "editorial-director",
    "editor-translator",
    "executive-producer",
    "guest",
    "host",
    "illustrator",
    "interviewer",
    "narrator",
    "organizer",
    "original-author",
    "performer",
    "producer",
    "recipient",
    "reviewed-author",
    "script-writer",
    "series-creator",
    "translator",
];

/// The date variables citationberg reads.
const DATE_VARIABLES: &[&str] = &[
    "accessed",
    "available-date",
    "event-date",
    "issued",
    "original-date",
    "submitted",
];

/// The parts of a name object citationberg reads as text.
const NAME_PARTS: &[&str] = &[
    "family",
    "given",
    "literal",
    "non-dropping-particle",
    "dropping-particle",
    "suffix",
];

/// Why an item can't be formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemProblem {
    /// Not valid CSL-JSON — a name that is a string, a date part that is
    /// not a number.
    Invalid(String),
    /// Valid CSL-JSON that hayagriva cannot render, such as a date range.
    Unsupported(String),
}

/// Rewrite a CSL-JSON item into the shape hayagriva reads without failing
/// or panicking.
///
/// Values hayagriva can't hold and never reads are dropped (`null`,
/// booleans, extension objects) or turned into text (floats, numbers past
/// `i64`, lists of strings, which are joined with ", "). Text is trimmed,
/// and blank text is an absent value. Name objects keep only their name
/// parts. Dates become a single `date-parts` date: an empty
/// date is dropped, a range whose end repeats its start is one date, a
/// month outside 1–12 or a day outside 1–31 is dropped (hayagriva printed
/// month 0 as "256"), and a `raw` ISO-like date is read into `date-parts`.
pub fn prepare(item: &Value) -> Result<Value, ItemProblem> {
    let Value::Object(fields) = item else {
        return Err(ItemProblem::Invalid("expected a CSL-JSON object".into()));
    };
    let mut prepared = Map::new();
    for (key, value) in fields {
        let value = if NAME_VARIABLES.contains(&key.as_str()) {
            names(key, value)?
        } else if DATE_VARIABLES.contains(&key.as_str()) {
            date(key, value)?
        } else {
            plain_value(value)
        };
        if let Some(value) = value {
            prepared.insert(key.clone(), value);
        }
    }
    Ok(Value::Object(prepared))
}

/// `item` with only the value types CSL-JSON defines, for an importer to
/// pass on: `null`s and booleans dropped, lists of strings joined with ", "
/// (the CSL form of `keyword`), null names and null name parts dropped,
/// numbers that aren't integers as text. Everything else — extension
/// objects, dates as written — is kept.
///
/// That is what validating through citationberg's `Item` used to guarantee
/// consumers of the CSL-JSON import, by rejecting the whole item instead.
pub fn well_typed(item: &Value) -> Value {
    let Value::Object(fields) = item else {
        return item.clone();
    };
    let mut kept = Map::new();
    for (key, value) in fields {
        let value = match value {
            Value::Null | Value::Bool(_) => None,
            Value::Number(n) if !n.is_i64() => Some(Value::String(n.to_string())),
            Value::Array(list) if NAME_VARIABLES.contains(&key.as_str()) => Some(Value::Array(
                list.iter()
                    .filter(|name| !name.is_null())
                    .map(|name| match name {
                        Value::Object(parts) => Value::Object(
                            parts
                                .iter()
                                .filter(|(_, part)| !part.is_null())
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect(),
                        ),
                        other => other.clone(),
                    })
                    .collect(),
            )),
            Value::Array(list) if list.iter().all(|v| v.is_string() || v.is_number()) => {
                let words: Vec<String> = list.iter().filter_map(text).collect();
                (!words.is_empty()).then(|| Value::String(words.join(", ")))
            }
            other => Some(other.clone()),
        };
        if let Some(value) = value {
            kept.insert(key.clone(), value);
        }
    }
    Value::Object(kept)
}

fn names(key: &str, value: &Value) -> Result<Option<Value>, ItemProblem> {
    let list = match value {
        Value::Null => return Ok(None),
        Value::Array(list) => list,
        _ => {
            return Err(ItemProblem::Invalid(format!(
                "{key}: expected a list of names"
            )))
        }
    };
    let mut names = Vec::with_capacity(list.len());
    for (i, name) in list.iter().enumerate() {
        match name {
            Value::Null => {}
            Value::Object(parts) => {
                let mut kept = Map::new();
                for (part, value) in parts {
                    if NAME_PARTS.contains(&part.as_str()) {
                        if let Some(text) = text(value) {
                            kept.insert(part.clone(), Value::String(text));
                        }
                    } else if part == "comma-suffix" && value.is_boolean() {
                        kept.insert(part.clone(), value.clone());
                    }
                }
                if kept.keys().any(|part| part != "comma-suffix") {
                    names.push(Value::Object(kept));
                }
            }
            _ => {
                return Err(ItemProblem::Invalid(format!(
                    "{key}[{i}]: a name must be an object"
                )))
            }
        }
    }
    Ok(Some(Value::Array(names)))
}

/// A date as `[year, month?, day?]`.
type DateParts = Vec<i64>;

fn date(key: &str, value: &Value) -> Result<Option<Value>, ItemProblem> {
    let fields = match value {
        Value::Null => return Ok(None),
        Value::Object(fields) => fields,
        _ => {
            return Err(ItemProblem::Invalid(format!(
                "{key}: expected a date object"
            )))
        }
    };

    let mut circa = fields.get("circa").is_some_and(is_true);
    let parts = match fields.get("date-parts") {
        Some(parts) => date_parts(key, parts)?,
        None => None,
    };
    let parts = match (parts, fields.get("raw")) {
        (Some(parts), _) => Some(parts),
        (None, Some(raw)) => {
            let (parts, raw_circa) = raw_date(key, raw)?;
            circa |= raw_circa;
            parts
        }
        (None, None) => None,
    };

    let Some(parts) = parts else {
        if fields
            .get("literal")
            .and_then(Value::as_str)
            .is_some_and(|l| !l.trim().is_empty())
        {
            return Err(ItemProblem::Unsupported(format!(
                "{key}: a date given only as literal text cannot be rendered"
            )));
        }
        return Ok(None);
    };

    let mut date = Map::new();
    date.insert("date-parts".into(), Value::from(vec![parts]));
    if let Some(season) = fields.get("season").and_then(text) {
        date.insert("season".into(), Value::String(season));
    }
    if circa {
        date.insert("circa".into(), Value::Bool(true));
    }
    Ok(Some(Value::Object(date)))
}

/// `date-parts`: one date, or a start and an end. A range whose end repeats
/// its start (or is empty) is that one date; a real range is unsupported.
fn date_parts(key: &str, value: &Value) -> Result<Option<DateParts>, ItemProblem> {
    let Value::Array(list) = value else {
        return Err(ItemProblem::Invalid(format!(
            "{key}: date-parts must be a list of dates"
        )));
    };
    let mut dates = Vec::with_capacity(list.len());
    for date in list {
        let Value::Array(components) = date else {
            return Err(ItemProblem::Invalid(format!(
                "{key}: each date in date-parts must be a list of numbers"
            )));
        };
        let mut numbers = Vec::with_capacity(components.len());
        for component in components {
            match component {
                Value::Null => {}
                Value::Number(n) => numbers.push(n.as_i64().ok_or_else(|| {
                    ItemProblem::Invalid(format!("{key}: date part {n} is not a whole number"))
                })?),
                Value::String(s) if s.trim().is_empty() => {}
                Value::String(s) => numbers.push(s.trim().parse().map_err(|_| {
                    ItemProblem::Invalid(format!("{key}: date part {s:?} is not a number"))
                })?),
                other => {
                    return Err(ItemProblem::Invalid(format!(
                        "{key}: date part {other} is not a number"
                    )))
                }
            }
        }
        if !numbers.is_empty() {
            dates.push(numbers);
        }
    }
    match dates.as_slice() {
        [] => Ok(None),
        [start] => valid_date(key, start).map(Some),
        [start, end] => {
            let start = valid_date(key, start)?;
            if valid_date(key, end)? == start {
                Ok(Some(start))
            } else {
                Err(range_unsupported(key))
            }
        }
        _ => Err(ItemProblem::Invalid(format!(
            "{key}: date-parts holds more than a start and an end"
        ))),
    }
}

/// `raw`: a year-first ISO-like date ("2019", "2019-05-03",
/// "2019-05-03T10:00:00Z"), optionally followed by `~` (circa) or by
/// `/` and an end date — the forms citationberg reads. Returns the date and
/// whether it is marked circa.
fn raw_date(key: &str, value: &Value) -> Result<(Option<DateParts>, bool), ItemProblem> {
    let Some(raw) = value.as_str() else {
        return Err(ItemProblem::Invalid(format!("{key}: raw must be text")));
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((None, false));
    }
    let unreadable =
        || ItemProblem::Unsupported(format!("{key}: cannot read the raw date {raw:?}"));
    let read = |side: &str| {
        let (numbers, circa) = raw_side(side).ok_or_else(unreadable)?;
        Ok((valid_date(key, &numbers).map_err(|_| unreadable())?, circa))
    };
    let mut sides = raw.split('/');
    let (start, start_circa) = read(sides.next().unwrap_or_default())?;
    match sides.next() {
        None => Ok((Some(start), start_circa)),
        Some(end) => {
            let (end, end_circa) = read(end)?;
            if sides.next().is_some() {
                return Err(unreadable());
            }
            if end == start {
                Ok((Some(start), start_circa || end_circa))
            } else {
                Err(range_unsupported(key))
            }
        }
    }
}

fn raw_side(side: &str) -> Option<(DateParts, bool)> {
    let mut numbers = Vec::with_capacity(3);
    let mut rest = side.trim();
    while numbers.len() < 3 {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            break;
        }
        numbers.push(rest[..digits].parse().ok()?);
        rest = &rest[digits..];
        match rest.strip_prefix('-') {
            Some(after) if after.starts_with(|c: char| c.is_ascii_digit()) => rest = after,
            _ => break,
        }
    }
    if numbers.is_empty() {
        return None;
    }
    Some((numbers, rest.starts_with('~')))
}

/// A date hayagriva can render: a year citationberg can hold, then a month
/// in 1–12 and a day in 1–31 if present — an out-of-range month or day is
/// dropped along with everything after it.
fn valid_date(key: &str, numbers: &[i64]) -> Result<DateParts, ItemProblem> {
    let year = numbers[0];
    if i16::try_from(year).is_err() {
        return Err(ItemProblem::Invalid(format!(
            "{key}: year {year} is out of range"
        )));
    }
    let mut date = vec![year];
    if let Some(&month) = numbers.get(1).filter(|m| (1..=12).contains(*m)) {
        date.push(month);
        if let Some(&day) = numbers.get(2).filter(|d| (1..=31).contains(*d)) {
            date.push(day);
        }
    }
    Ok(date)
}

fn range_unsupported(key: &str) -> ItemProblem {
    ItemProblem::Unsupported(format!(
        "{key}: date ranges cannot be rendered — pass the start date alone to format it"
    ))
}

/// CSL-JSON's `circa`, read the way citationberg reads it.
fn is_true(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::String(s) => s == "true",
        Value::Number(n) => n.as_u64() == Some(1),
        _ => false,
    }
}

/// A string or number as text, trimmed; blank text is no text.
///
/// Whitespace around a value is never part of it, and a blank value printed
/// its style's labels with nothing in them ("2020., ."). It was also the
/// realistic way into a hayagriva panic — after a prefix ending in a space,
/// a value opening with whitespace and then a multibyte character
/// (`"Vol.&#160;"` then `" é"`) — which `vendor/hayagriva` now fixes.
fn text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(s) => s.trim(),
        Value::Number(n) => return Some(n.to_string()),
        _ => return None,
    };
    (!text.is_empty()).then(|| text.to_owned())
}

/// Any variable that is neither a name nor a date.
fn plain_value(value: &Value) -> Option<Value> {
    match value {
        Value::Number(n) if n.is_i64() => Some(value.clone()),
        Value::String(_) | Value::Number(_) => text(value).map(Value::String),
        Value::Array(list) => {
            if !list.iter().all(|v| v.is_string() || v.is_number()) {
                return None;
            }
            let words: Vec<String> = list.iter().filter_map(text).collect();
            (!words.is_empty()).then(|| Value::String(words.join(", ")))
        }
        Value::Null | Value::Bool(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn issued(date: Value) -> Result<Option<Value>, ItemProblem> {
        prepare(&json!({ "issued": date })).map(|item| item.get("issued").cloned())
    }

    #[test]
    fn dates_become_one_valid_date_parts_date() {
        let parts = |v: Value| issued(v).unwrap();
        assert_eq!(
            parts(json!({"date-parts": [["2019", "5", "3"]]})),
            Some(json!({"date-parts": [[2019, 5, 3]]}))
        );
        assert_eq!(
            parts(json!({"raw": "2018-07-15T10:00:00Z"})),
            Some(json!({"date-parts": [[2018, 7, 15]]}))
        );
        assert_eq!(
            parts(json!({"raw": "2019-05~"})),
            Some(json!({"date-parts": [[2019, 5]], "circa": true}))
        );
        assert_eq!(
            parts(json!({"date-parts": [[2019]], "season": 2, "circa": "true"})),
            Some(json!({"date-parts": [[2019]], "season": "2", "circa": true}))
        );
        assert_eq!(
            parts(json!({"date-parts": [[2019, 13, 3]]})),
            Some(json!({"date-parts": [[2019]]}))
        );
        assert_eq!(
            parts(json!({"date-parts": [[], [2020]]})),
            Some(json!({"date-parts": [[2020]]}))
        );
        assert_eq!(
            parts(json!({"date-parts": [[2020]], "raw": "1999"})),
            Some(json!({"date-parts": [[2020]]}))
        );
        assert_eq!(
            parts(json!({"date-parts": [[]], "raw": "1999"})),
            Some(json!({"date-parts": [[1999]]}))
        );
        assert_eq!(parts(json!({"date-parts": [[null]]})), None);
        assert_eq!(parts(Value::Null), None);
    }

    #[test]
    fn unreadable_dates_are_refused_with_the_variable_named() {
        assert!(
            matches!(issued(json!({"raw": "May 2019"})), Err(ItemProblem::Unsupported(m)) if m.contains("issued"))
        );
        assert!(matches!(
            issued(json!({"raw": "2019/2020/2021"})),
            Err(ItemProblem::Unsupported(_))
        ));
        assert!(matches!(
            issued(json!({"date-parts": [["n.d."]]})),
            Err(ItemProblem::Invalid(_))
        ));
        assert!(matches!(
            issued(json!({"date-parts": [[2019.5]]})),
            Err(ItemProblem::Invalid(_))
        ));
        assert!(matches!(
            issued(json!({"date-parts": [[40000]]})),
            Err(ItemProblem::Invalid(_))
        ));
        assert!(matches!(
            issued(json!({"date-parts": [[1], [2], [3]]})),
            Err(ItemProblem::Invalid(_))
        ));
        assert!(matches!(
            issued(json!({"date-parts": 2019})),
            Err(ItemProblem::Invalid(_))
        ));
        assert!(matches!(
            issued(json!("2019")),
            Err(ItemProblem::Invalid(_))
        ));
    }

    #[test]
    fn names_keep_only_their_name_parts() {
        let item = prepare(&json!({
            "author": [
                {"family": "Doe", "given": null, "sequence": "first", "comma-suffix": true, "suffix": "Jr."},
                null,
                {"literal": 42},
                {"affiliation": []}
            ]
        }))
        .unwrap();
        assert_eq!(
            item["author"],
            json!([{"family": "Doe", "comma-suffix": true, "suffix": "Jr."}, {"literal": "42"}])
        );
    }

    #[test]
    fn other_variables_keep_strings_and_integers_only() {
        let item = prepare(&json!({
            "title": "T", "volume": 4, "issue": 1.5, "page": u64::MAX,
            "keyword": ["a", "", 3], "categories": [{"x": 1}],
            "note": null, "custom": {"eprint": {}}, "suppress-author": true
        }))
        .unwrap();
        assert_eq!(
            item,
            json!({"title": "T", "volume": 4, "issue": "1.5", "page": u64::MAX.to_string(), "keyword": "a, 3"})
        );
    }
}
