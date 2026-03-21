/// Protected acronyms that should NOT be case-modified.
const PROTECTED_ACRONYMS: &[&str] = &[
    "IBGE", "IPEA", "MEC", "CAPES", "CNPq", "USP", "UFRJ", "UFMG",
    "UNESP", "UFSC", "INPE", "FIOCRUZ", "EMBRAPA", "UNICAMP",
    "UNESCO", "WHO", "OMS", "UNICEF", "ONU", "NASA", "NIST", "NIH",
    "IEEE", "ACM", "OECD",
];

/// Check if text is a protected acronym.
pub fn is_protected_acronym(text: &str) -> bool {
    let clean = text.replace(['.', ','], "").trim().to_string();
    PROTECTED_ACRONYMS.iter().any(|&a| a.eq_ignore_ascii_case(&clean))
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
    if let Some(comma_pos) = text.find(',') {
        let before_comma = &text[..comma_pos];

        if is_protected_acronym(before_comma.trim()) {
            return text.to_string();
        }

        let trimmed = before_comma.trim();
        if trimmed.is_empty() || trimmed.chars().next().map_or(true, |c| c.is_ascii_digit()) {
            return text.to_string();
        }

        let uppercased = before_comma.to_uppercase();
        format!("{uppercased}{}", &text[comma_pos..])
    } else {
        text.to_string()
    }
}

fn post_process_in_text(text: &str) -> String {
    let mut result = text.to_string();

    if let (Some(open), Some(close)) = (result.find('('), result.rfind(')')) {
        let inner = &result[open + 1..close];
        let mut new_inner = String::new();

        for part in inner.split(';') {
            if !new_inner.is_empty() {
                new_inner.push(';');
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
                new_inner.push_str(part);
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
        assert!(result.starts_with("SILVA"), "family name should be uppercased: {result}");
    }

    #[test]
    fn test_abnt_protects_acronyms() {
        let input = "IBGE (2024). Census data.";
        let result = post_process_abnt(input, false);
        assert!(result.starts_with("IBGE"), "IBGE should stay IBGE: {result}");
    }

    #[test]
    fn test_abnt_in_text() {
        let input = "(Silva, 2024)";
        let result = post_process_abnt(input, true);
        assert!(result.contains("SILVA"), "in-text family name should be uppercased: {result}");
    }
}
