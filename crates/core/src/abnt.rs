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
    let mut result = text.to_string();

    if let (Some(open), Some(close)) = (result.find('('), result.rfind(')')) {
        let inner = &result[open + 1..close];
        let mut new_inner = String::new();

        for part in inner.split(';') {
            if !new_inner.is_empty() {
                new_inner.push_str("; ");
            }

            let part = part.trim();
            if let Some(comma_pos) = part.find(',') {
                let name = part[..comma_pos].trim();
                if is_protected_acronym(name) {
                    new_inner.push_str(part);
                } else {
                    new_inner.push_str(&name.to_uppercase());
                    new_inner.push_str(&part[comma_pos..]);
                }
            } else {
                // No comma — could be a single name, "et al.", or a year
                let trimmed = part.trim();
                let is_name = !trimmed.is_empty()
                    && trimmed
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() && c.is_uppercase())
                    && !trimmed.contains('.')
                    && !trimmed.chars().any(|c| c.is_ascii_digit());
                if is_name && !is_protected_acronym(trimmed) {
                    new_inner.push_str(&trimmed.to_uppercase());
                } else {
                    new_inner.push_str(part);
                }
            }
        }

        let prefix = &text[..open];
        let suffix = &text[close + 1..];
        result = format!("{prefix}({new_inner}){suffix}");
    }

    result
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
