/// Protected acronyms that should NOT be case-modified.
const PROTECTED_ACRONYMS: &[&str] = &[
    "IBGE", "IPEA", "MEC", "CAPES", "CNPq", "USP", "UFRJ", "UFMG", "UNESP", "UFSC", "INPE",
    "FIOCRUZ", "EMBRAPA", "UNICAMP", "UNESCO", "WHO", "OMS", "UNICEF", "ONU", "NASA", "NIST",
    "NIH", "IEEE", "ACM", "OECD",
];

/// Check if text is a protected acronym.
pub fn is_protected_acronym(text: &str) -> bool {
    let clean = text.replace(['.', ','], "").trim().to_string();
    PROTECTED_ACRONYMS
        .iter()
        .any(|&a| a.eq_ignore_ascii_case(&clean))
}

/// Post-process citation text for ABNT 2023 compliance.
///
/// For references (is_in_text=false): uppercases the family name before the comma.
/// For in-text (is_in_text=true): uppercases names inside parentheses.
///
/// Protects institutional acronyms from modification.
pub fn post_process_abnt(text: &str, is_in_text: bool) -> String {
    if text.is_empty() {
        return String::new();
    }

    if is_in_text {
        post_process_in_text(text)
    } else {
        post_process_reference(text)
    }
}

fn post_process_reference(text: &str) -> String {
    // In ABNT references, the author's family name comes first, before a comma.
    // "Silva, J. da" → "SILVA, J. da"
    //
    // Only uppercase if the text before the first comma is a plain family name:
    // - No periods (period indicates institutional author like "IBGE." or sentence)
    // - No HTML tags (the family name is always plain text)
    // - Not a protected acronym
    if let Some(comma_pos) = find_first_text_comma(text) {
        let before_comma = &text[..comma_pos];
        let plain_before = strip_html_tags(before_comma).trim().to_string();

        // Skip if it contains a period — institutional author or sentence structure
        if plain_before.contains('.') {
            return text.to_string();
        }

        if is_protected_acronym(&plain_before) {
            return text.to_string();
        }

        if plain_before.is_empty()
            || plain_before
                .chars()
                .next()
                .is_none_or(|c| c.is_ascii_digit())
        {
            return text.to_string();
        }

        // Only uppercase the family name text before the comma, preserving HTML
        let uppercased = uppercase_text_only(before_comma);
        format!("{uppercased}{}", &text[comma_pos..])
    } else {
        text.to_string()
    }
}

/// Find the first comma that is outside of any HTML tag.
fn find_first_text_comma(text: &str) -> Option<usize> {
    let mut in_tag = false;
    for (i, c) in text.char_indices() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            ',' if !in_tag => return Some(i),
            _ => {}
        }
    }
    None
}

/// Strip all HTML tags from a string, leaving only text content.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result
}

/// Uppercase only text content in a string, preserving HTML tags as-is.
fn uppercase_text_only(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                result.push(c);
            }
            '>' => {
                in_tag = false;
                result.push(c);
            }
            _ if in_tag => result.push(c),
            _ => {
                for uc in c.to_uppercase() {
                    result.push(uc);
                }
            }
        }
    }
    result
}

fn post_process_in_text(text: &str) -> String {
    let (Some(open), Some(close)) = (text.find('('), text.rfind(')')) else {
        return text.to_string();
    };
    // A ")" before the first "(" is not a parenthesized citation.
    if close < open {
        return text.to_string();
    }

    let inner = &text[open + 1..close];
    let mut new_inner = String::new();

    for part in split_outside_tags(inner, ';') {
        if !new_inner.is_empty() {
            new_inner.push_str("; ");
        }

        let part = part.trim();
        if let Some(comma_pos) = find_first_text_comma(part) {
            let name = part[..comma_pos].trim();
            if is_protected_acronym(&strip_html_tags(name)) {
                new_inner.push_str(part);
            } else {
                new_inner.push_str(&uppercase_name(name));
                new_inner.push_str(&part[comma_pos..]);
            }
        } else {
            // No comma — could be a single name, "et al.", or a year
            let plain = strip_html_tags(part);
            let trimmed = plain.trim();
            let is_name = !trimmed.is_empty()
                && trimmed
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() && c.is_uppercase())
                && !trimmed.contains('.')
                && !trimmed.chars().any(|c| c.is_ascii_digit());
            if is_name && !is_protected_acronym(trimmed) {
                new_inner.push_str(&uppercase_name(part));
            } else {
                new_inner.push_str(part);
            }
        }
    }

    format!("{}({new_inner}){}", &text[..open], &text[close + 1..])
}

/// Split on `separator` outside HTML tags — a style's markup carries `;`
/// in its CSS (`style="font-variant: small-caps;"`).
fn split_outside_tags(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_tag = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if c == separator && !in_tag => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Uppercase a name in an ABNT citation, leaving markup and a following
/// "et al." as they are: NBR 10520 prints "et al." in lower case.
fn uppercase_name(name: &str) -> String {
    match find_et_al(name) {
        Some(at) => format!("{}{}", uppercase_text_only(&name[..at]), &name[at..]),
        None => uppercase_text_only(name),
    }
}

/// Byte offset of a standalone "et al" in any case — not the one inside
/// "Bennet Alvarez".
fn find_et_al(text: &str) -> Option<usize> {
    // ASCII lowercasing keeps every byte offset valid in `text`.
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("et al") {
        let at = from + found;
        let starts_word = lower[..at]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || c == '>');
        let ends_word = lower[at + 5..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if starts_word && ends_word {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abnt_reference_uppercase_family() {
        let input = "Silva, J. (2024). Title.";
        let result = post_process_abnt(input, false);
        assert!(
            result.starts_with("SILVA"),
            "family name should be uppercased: {result}"
        );
    }

    #[test]
    fn test_abnt_reference_preserves_html() {
        let input =
            r#"Silva, João. <span style="font-weight: bold;">Title</span>. Publisher, 2024."#;
        let result = post_process_abnt(input, false);
        assert!(
            result.starts_with("SILVA"),
            "family name should be uppercased: {result}"
        );
        assert!(
            result.contains(r#"<span style="font-weight: bold;">"#),
            "HTML tags must NOT be uppercased: {result}"
        );
    }

    #[test]
    fn test_abnt_protects_acronyms() {
        let input = "IBGE (2024). Census data.";
        let result = post_process_abnt(input, false);
        assert!(
            result.starts_with("IBGE"),
            "IBGE should stay IBGE: {result}"
        );
    }

    #[test]
    fn test_abnt_institutional_no_comma() {
        // Institutional author with no comma — should not uppercase the rest
        let input = r#"IBGE. <span style="font-weight: bold;">Censo</span>. Rio de Janeiro, 2024."#;
        let result = post_process_abnt(input, false);
        assert!(
            result.contains("Censo"),
            "content after institutional name should not be uppercased: {result}"
        );
    }

    #[test]
    fn test_abnt_in_text() {
        let input = "(Silva, 2024)";
        let result = post_process_abnt(input, true);
        assert!(
            result.contains("SILVA"),
            "in-text family name should be uppercased: {result}"
        );
    }

    #[test]
    fn in_text_parentheses_out_of_order_are_left_alone() {
        // `&result[open + 1..close]` panicked when the first ")" came
        // before the first "(" — any style's in-text with ABNT on.
        assert_eq!(post_process_abnt("a) (b", true), "a) (b");
        assert_eq!(post_process_abnt("Smith, x) y (z", true), "Smith, x) y (z");
    }

    #[test]
    fn et_al_stays_in_lower_case() {
        // NBR 10520 prints "et al." in lower case; it read as part of the
        // family name and came out "(SILVA ET AL., 2024)".
        assert_eq!(
            post_process_abnt("(Silva et al., 2024)", true),
            "(SILVA et al., 2024)"
        );
        assert_eq!(
            post_process_abnt(
                r#"(Silva <span style="font-style: italic;">et al.</span>, 2024)"#,
                true
            ),
            r#"(SILVA <span style="font-style: italic;">et al.</span>, 2024)"#
        );
        assert_eq!(
            post_process_abnt("(Souza; Silva et al., 2024)", true),
            "(SOUZA; SILVA et al., 2024)"
        );
        // Only the standalone words: "Bennet Alvarez" holds "et al" too.
        assert_eq!(
            post_process_abnt("(Bennet Alvarez, 2024)", true),
            "(BENNET ALVAREZ, 2024)"
        );
    }

    #[test]
    fn in_text_markup_is_not_uppercased() {
        assert_eq!(
            post_process_abnt(
                r#"(<span style="font-variant: small-caps;">Silva</span>, 2024)"#,
                true
            ),
            r#"(<span style="font-variant: small-caps;">SILVA</span>, 2024)"#
        );
    }

    #[test]
    fn test_abnt_in_text_multi_author() {
        let input = "(Souza; Santos, 2023)";
        let result = post_process_abnt(input, true);
        assert!(
            result.contains("SOUZA"),
            "first author should be uppercased: {result}"
        );
        assert!(
            result.contains("SANTOS"),
            "second author should be uppercased: {result}"
        );
        assert!(
            result.contains("; "),
            "semicolons should have proper spacing: {result}"
        );
    }
}
