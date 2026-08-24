//! 纯文本逻辑：拼音首字母、可搜索内容组装、FTS 查询构建（与数据库无关）。

// ---------------------------------------------------------------------------
// 拼音首字母与可搜索内容
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

/// 拼接可搜索内容：`备注 账户名 备注拼音 账户名拼音`。
/// 空字段跳过；所有字段为空时返回空串（仍保留文档行）。
pub fn build_search_content(note: Option<&str>, account_name: &str) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    let text_parts = [note, Some(account_name)];
    for text in text_parts.into_iter().flatten() {
        let text = text.trim();
        if !text.is_empty() {
            parts.push(text.to_string());
        }
    }
    for text in text_parts.into_iter().flatten() {
        let initials = pinyin_initials(text);
        if !initials.is_empty() {
            parts.push(initials);
        }
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------
// 查询构建
// ---------------------------------------------------------------------------

/// FTS5 查询中需引号包裹才视为字面量的字符（除 `"` 与 `*` 另有处理外，
/// 引号包裹已覆盖 AND/OR/NOT/NEAR/括号/连字符/冒号/脱字符/加号等全部特殊语法）。
/// `"` 在 FTS5 短语中无法转义（实测 `""` 双写不支持），直接剥离；
/// `*` 剥离以避免用户手输通配符干扰（前缀通配由本函数统一附加）。
///
/// 按空白分词；每个词条生成 `"词条"` 与 `"词条"*`（前缀通配）两个变体并 OR，
/// 词条之间 AND 连接。如 `cf 午餐` → `("cf" OR "cf"*) AND ("午餐" OR "午餐"*)`。
/// 空查询返回空串，调用方应直接返回空结果。
pub fn build_match_query(query: &str) -> String {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|&c| c != '"' && c != '*')
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return String::new();
    }
    terms
        .iter()
        .map(|t| format!("(\"{t}\" OR \"{t}\"*)"))
        .collect::<Vec<_>>()
        .join(" AND ")
}
