//! 纯文本逻辑：统一模糊搜索语义（issue #195 规格，issue #196 全局搜索侧实现）。
//!
//! 统一语义契约（前后端共同遵守，规格以 issue #195 为唯一真源）：
//! 输入按空白切词，词条之间 AND；每个词条对每个可搜索字段（全局搜索：备注、
//! 转出账户名）判定——
//! **命中 = 原文连续子串（大小写不敏感）∨ 词条是该字段拼音首字母串的子序列
//! （大小写不敏感）**
//!
//! 混合输入（如「招zsyh」）无需特判：含汉字的词条对纯 ASCII 首字母串的子序列
//! 匹配必然失败，自然落到原文子串路径，两条路径任一命中即算。

// ---------------------------------------------------------------------------
// 拼音首字母
// ---------------------------------------------------------------------------

/// 常见多音字在记账语境下的读音修正（前字 + 当前字 → 拼音首字母）。
/// `pinyin` crate 按单字常用读音取音，无上下文消歧；此处用简单前字规则覆盖
/// 高频金融/账户场景的例外读音，其余多音字沿用默认读音（已知局限）。
fn polyphone_initial(prev: Option<char>, ch: char) -> Option<char> {
    match ch {
        // 行：银行/商业银行等 → háng（h）；默认行走/行为 → xíng（x）
        '行' if prev == Some('银') => Some('h'),
        _ => None,
    }
}

/// 生成拼音首字母缩写（小写）。逐字符处理：
/// - 中文字符取拼音（无声调）首字母，如「招商银行」→ `zsyh`（银行 → yh，多音字修正）；
/// - ASCII 字母/数字小写保留（如 `ABC银行` → `abcyh`，`123` → `123`）；
/// - 其余字符（标点、空格等）跳过。
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

// ---------------------------------------------------------------------------
// 统一语义匹配
// ---------------------------------------------------------------------------

/// 大小写不敏感的子序列判定：`pattern` 的每个字符按原顺序出现在 `target` 中
/// （允许跳字，间隔不限）。空 pattern 恒命中（`all` 对空迭代器为 true）。
pub fn is_subsequence(pattern: &str, target: &str) -> bool {
    let target_lower = target.to_lowercase();
    let mut target_chars = target_lower.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| target_chars.any(|t| t == p))
}

/// 词条对单个可搜索字段判定（统一语义契约）：
/// 原文连续子串（大小写不敏感）∨ 该字段拼音首字母串的子序列（大小写不敏感）。
pub fn term_matches_text(term: &str, text: &str) -> bool {
    text.to_lowercase().contains(&term.to_lowercase())
        || is_subsequence(term, &pinyin_initials(text))
}

/// 词条对一笔交易判定：任一可搜索字段（备注 ∨ 转出账户名）命中即算。
/// 两条路径任一命中即算，字段之间 OR、词条之间 AND（由调用方组合）。
pub fn term_matches(term: &str, note: Option<&str>, account_name: &str) -> bool {
    note.is_some_and(|n| term_matches_text(term, n)) || term_matches_text(term, account_name)
}

/// 切词：按空白拆分，过滤空词条（词条之间 AND 由调用方组合）。
pub fn split_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_string).collect()
}
