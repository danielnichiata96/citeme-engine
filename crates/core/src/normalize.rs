//! CSL XML normalization — workarounds for Hayagriva rendering bugs.
//!
//! Hayagriva (≤ 0.9) renders the `delimiter` attribute of a `<date>` element
//! between every `<date-part>` regardless of whether a neighboring part is
//! empty. That produces trailing-space artifacts for year-only dates:
//!
//!   `<date form="text" delimiter=" ">`
//!     `<date-part name="year"/>`
//!     `<date-part name="month" form="short"/>`
//!     `<date-part name="day"/>`
//!   `</date>`
//!
//! With year-only input, Hayagriva outputs `"2018 "` (trailing space) because
//! it emits the delimiter between every date-part pair even when the right-hand
//! part is empty. When the parent group then appends a suffix like `;`, the
//! result becomes `"2018 ;"` — a broken punctuation artifact.
//!
//! Hayagriva *does* correctly suppress `prefix` on a `<date-part>` whose prior
//! siblings are all empty, so the workaround is to rewrite the equivalent
//! `delimiter` usage as per-part `prefix`:
//!
//!   `<date form="text">`
//!     `<date-part name="year"/>`
//!     `<date-part name="month" form="short" prefix=" "/>`
//!     `<date-part name="day" prefix=" "/>`
//!   `</date>`
//!
//! This normalization is applied to every CSL style and locale XML before it
//! reaches Hayagriva. It is idempotent: running it twice is a no-op.
//!
//! TODO(hayagriva): remove this module once Hayagriva fixes empty-sibling
//! delimiter handling upstream. See repro in tests below.

/// Normalize CSL XML by rewriting `<date delimiter="X">…<date-part/>…</date>`
/// into `<date>…<date-part prefix="X"/>…</date>` for all but the first
/// `<date-part>`. `<date-part>` elements that already carry a `prefix` are
/// left untouched (the style author's explicit intent wins).
///
/// This is a narrow string transform — it does NOT parse arbitrary XML, only
/// the exact `<date>` / `<date-part>` patterns defined by CSL 1.0.2. Non-CSL
/// XML and malformed input are returned unchanged or best-effort preserved.
pub fn normalize_csl_xml(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;

    while let Some(open_rel) = find_date_element_start(rest) {
        out.push_str(&rest[..open_rel]);
        rest = &rest[open_rel..];

        let tag_end = match find_tag_end(rest) {
            Some(i) => i,
            None => {
                out.push_str(rest);
                return out;
            }
        };
        let open_tag = &rest[..tag_end];
        let self_closing = open_tag.ends_with("/>");

        let (rewritten_open, delim) = strip_delimiter_attr(open_tag);
        out.push_str(&rewritten_open);

        if self_closing {
            rest = &rest[tag_end..];
            continue;
        }

        let body_start = tag_end;
        let close_rel = match rest[body_start..].find("</date>") {
            Some(i) => i,
            None => {
                out.push_str(&rest[body_start..]);
                return out;
            }
        };
        let body = &rest[body_start..body_start + close_rel];

        match delim {
            Some(d) => out.push_str(&inject_date_part_prefixes(body, &d)),
            None => out.push_str(body),
        }
        out.push_str("</date>");
        rest = &rest[body_start + close_rel + "</date>".len()..];
    }

    out.push_str(rest);
    out
}

/// Find the next `<date` opening that is NOT `<date-part`. Returns the byte
/// offset of the `<` character.
fn find_date_element_start(xml: &str) -> Option<usize> {
    let bytes = xml.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = xml[start..].find("<date") {
        let abs = start + rel;
        match bytes.get(abs + 5).copied() {
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/') => {
                return Some(abs);
            }
            _ => start = abs + 5,
        }
    }
    None
}

/// Given a slice starting at `<`, return the byte index one past the matching
/// `>` (respecting quoted attribute values). Returns `None` if malformed.
fn find_tag_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut in_quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match in_quote {
            Some(q) if b == q => in_quote = None,
            Some(_) => {}
            None => {
                if b == b'"' || b == b'\'' {
                    in_quote = Some(b);
                } else if b == b'>' {
                    return Some(i + 1);
                }
            }
        }
        i += 1;
    }
    None
}

/// Remove a single `delimiter="…"` (or `delimiter='…'`) attribute from an
/// opening tag, returning the rewritten tag and the extracted delimiter value.
fn strip_delimiter_attr(tag: &str) -> (String, Option<String>) {
    let bytes = tag.as_bytes();
    let mut i = 0usize;
    while i + "delimiter".len() < bytes.len() {
        if !tag[i..].starts_with("delimiter") {
            i += 1;
            continue;
        }
        let key_end = i + "delimiter".len();
        if i > 0 && !is_attr_boundary(bytes[i - 1]) {
            i += 1;
            continue;
        }
        let after_key = &tag[key_end..];
        let after_trim = after_key.trim_start();
        let ws_skipped = after_key.len() - after_trim.len();
        let rest_after_key = &tag[key_end + ws_skipped..];
        if !rest_after_key.starts_with('=') {
            i += 1;
            continue;
        }
        let after_eq = &rest_after_key[1..];
        let after_eq_trim = after_eq.trim_start();
        let ws_after_eq = after_eq.len() - after_eq_trim.len();
        let quote_pos = key_end + ws_skipped + 1 + ws_after_eq;
        let quote = bytes.get(quote_pos).copied();
        if quote != Some(b'"') && quote != Some(b'\'') {
            i += 1;
            continue;
        }
        let value_start = quote_pos + 1;
        let close_rel = match tag[value_start..].find(quote.unwrap() as char) {
            Some(k) => k,
            None => return (tag.to_string(), None),
        };
        let value = &tag[value_start..value_start + close_rel];
        let end = value_start + close_rel + 1;

        // Also consume one leading whitespace before the attribute, if any, to
        // keep the tag tidy.
        let remove_from = if i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            i - 1
        } else {
            i
        };
        let mut rewritten = String::with_capacity(tag.len());
        rewritten.push_str(&tag[..remove_from]);
        rewritten.push_str(&tag[end..]);
        return (rewritten, Some(value.to_string()));
    }
    (tag.to_string(), None)
}

/// True if `b` is a valid character preceding an attribute name (i.e. the
/// previous char must be whitespace or the opening tag name separator).
fn is_attr_boundary(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// For each `<date-part …/>` (or `<date-part …></date-part>`) inside `body`,
/// inject `prefix="{delim}"` on every occurrence EXCEPT the first, and ONLY
/// when the element does not already declare a `prefix` attribute.
fn inject_date_part_prefixes(body: &str, delim: &str) -> String {
    let mut out = String::with_capacity(body.len() + 32);
    let mut rest = body;
    let mut index = 0usize;

    while let Some(rel) = rest.find("<date-part") {
        out.push_str(&rest[..rel]);
        rest = &rest[rel..];

        // Only treat as a <date-part> element if the next char is whitespace,
        // `>`, or `/` — never a letter (guards against hypothetical `<date-partx>`).
        let after_name = rest.as_bytes().get(10).copied();
        let is_real_tag = matches!(
            after_name,
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
        );
        if !is_real_tag {
            out.push_str(&rest[..10]);
            rest = &rest[10..];
            continue;
        }

        let tag_end = match find_tag_end(rest) {
            Some(i) => i,
            None => {
                out.push_str(rest);
                return out;
            }
        };
        let tag = &rest[..tag_end];
        if index == 0 || has_prefix_attr(tag) {
            out.push_str(tag);
        } else {
            let self_closing = tag.ends_with("/>");
            let cut = if self_closing { tag.len() - 2 } else { tag.len() - 1 };
            out.push_str(&tag[..cut]);
            out.push_str(" prefix=\"");
            // The delimiter value was extracted verbatim from a quoted XML
            // attribute — it is already entity-encoded. Re-emit as-is.
            out.push_str(delim);
            out.push('"');
            out.push_str(&tag[cut..]);
        }
        index += 1;
        rest = &rest[tag_end..];
    }
    out.push_str(rest);
    out
}

fn has_prefix_attr(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = tag[i..].find("prefix") {
        let abs = i + rel;
        let prev_ok = abs == 0 || is_attr_boundary(bytes[abs - 1]);
        let next = bytes.get(abs + 6).copied();
        let next_ok = matches!(
            next,
            Some(b'=') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
        );
        if prev_ok && next_ok {
            let after = tag[abs + 6..].trim_start();
            if after.starts_with('=') {
                return true;
            }
        }
        i = abs + 6;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_date_delimiter_into_date_part_prefix() {
        let xml = r#"<date form="text" delimiter=" ">
  <date-part name="year"/>
  <date-part name="month" form="short" strip-periods="true"/>
  <date-part name="day"/>
</date>"#;
        let out = normalize_csl_xml(xml);
        assert!(!out.contains("delimiter="), "delimiter attr should be stripped: {out}");
        assert!(out.contains(r#"<date-part name="year"/>"#), "first date-part unchanged: {out}");
        assert!(
            out.contains(r#"<date-part name="month" form="short" strip-periods="true" prefix=" "/>"#),
            "month gets prefix: {out}"
        );
        assert!(
            out.contains(r#"<date-part name="day" prefix=" "/>"#),
            "day gets prefix: {out}"
        );
    }

    #[test]
    fn passthrough_when_no_delimiter() {
        let xml = r#"<date form="text"><date-part name="year"/></date>"#;
        assert_eq!(normalize_csl_xml(xml), xml);
    }

    #[test]
    fn does_not_touch_date_part_delimiter() {
        // <date-part> can also have a delimiter (for range separators); leave it.
        let xml = r#"<date><date-part name="year" delimiter="-"/></date>"#;
        assert_eq!(normalize_csl_xml(xml), xml);
    }

    #[test]
    fn preserves_existing_prefix() {
        let xml = r#"<date delimiter=" ">
  <date-part name="year"/>
  <date-part name="month" prefix="@"/>
</date>"#;
        let out = normalize_csl_xml(xml);
        assert!(out.contains(r#"<date-part name="month" prefix="@"/>"#), "existing prefix wins: {out}");
        assert!(!out.contains("delimiter="));
    }

    #[test]
    fn idempotent() {
        let xml = r#"<date form="text" delimiter=" ">
  <date-part name="year"/>
  <date-part name="month" form="short"/>
</date>"#;
        let once = normalize_csl_xml(xml);
        let twice = normalize_csl_xml(&once);
        assert_eq!(once, twice, "second run should be a no-op");
    }

    #[test]
    fn handles_self_closing_date() {
        let xml = r#"<date variable="issued" form="numeric"/>"#;
        assert_eq!(normalize_csl_xml(xml), xml);
    }

    #[test]
    fn handles_single_quoted_delimiter() {
        let xml = r#"<date delimiter=' '>
  <date-part name="year"/>
  <date-part name="month"/>
</date>"#;
        let out = normalize_csl_xml(xml);
        assert!(!out.contains("delimiter="), "delimiter stripped: {out}");
        assert!(out.contains(r#"<date-part name="month" prefix=" "/>"#), "month gets prefix: {out}");
    }

    #[test]
    fn multiple_date_elements_in_one_document() {
        let xml = r#"<style>
<date delimiter=" "><date-part name="year"/><date-part name="month"/></date>
<other>text</other>
<date delimiter="/"><date-part name="day"/><date-part name="year"/></date>
</style>"#;
        let out = normalize_csl_xml(xml);
        assert!(out.contains(r#"<date-part name="month" prefix=" "/>"#), "first date rewritten: {out}");
        assert!(out.contains(r#"<date-part name="year" prefix="/"/>"#), "second date rewritten: {out}");
    }

    #[test]
    fn leaves_non_matching_elements_alone() {
        let xml = r#"<text variable="issued" delimiter=" "/>"#;
        assert_eq!(normalize_csl_xml(xml), xml);
    }

    #[test]
    fn escapes_special_chars_in_delimiter() {
        let xml = r#"<date delimiter="&amp;">
  <date-part name="year"/>
  <date-part name="month"/>
</date>"#;
        let out = normalize_csl_xml(xml);
        assert!(
            out.contains(r#"prefix="&amp;""#),
            "delimiter value preserved entity-encoded: {out}"
        );
    }
}
