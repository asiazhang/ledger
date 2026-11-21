fn polyphone_initial(prev: Option<char>, ch: char) -> Option<char> {
    match ch {
        '行' if prev == Some('银') => Some('h'),
        _ => None,
    }
}

pub fn pinyin_initials(text: &str) -> String {
    use pinyin::ToPinyin;
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        if let Some(first) = polyphone_initial(prev, ch) {
            out.push(first);
        } else if let Some(py) = ch.to_pinyin() {
            if let Some(first) = py.plain().chars().next() {
                out.push(first.to_ascii_lowercase());
            }
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        }
        prev = Some(ch);
    }
    out
}

pub fn is_subsequence(pattern: &str, target: &str) -> bool {
    let target_lower = target.to_lowercase();
    let mut target_chars = target_lower.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| target_chars.any(|t| t == p))
}

pub fn term_matches_text(term: &str, text: &str) -> bool {
    text.to_lowercase().contains(&term.to_lowercase())
        || is_subsequence(term, &pinyin_initials(text))
}

pub fn term_matches(
    term: &str,
    note: Option<&str>,
    account_name: &str,
    merchant_name: Option<&str>,
) -> bool {
    note.is_some_and(|n| term_matches_text(term, n))
        || term_matches_text(term, account_name)
        || merchant_name.is_some_and(|m| term_matches_text(term, m))
}

pub fn split_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_string).collect()
}
