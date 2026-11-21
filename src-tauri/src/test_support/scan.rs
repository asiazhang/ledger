fn raw_string_open_quote(chars: &[char], i: usize) -> Option<usize> {
    let n = chars.len();
    if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        return None;
    }
    let mut j = i;
    if j + 1 < n && chars[j] == 'b' && chars[j + 1] == 'r' {
        j += 2;
    } else if j < n && chars[j] == 'r' {
        j += 1;
    } else {
        return None;
    }
    while j < n && chars[j] == '#' {
        j += 1;
    }
    if j < n && chars[j] == '"' {
        Some(j)
    } else {
        None
    }
}

pub fn mask_non_code(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let n = bytes.len();
    let mut out = bytes.clone();
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for k in out.iter_mut().take(to.min(n)).skip(from) {
            if *k != '\n' {
                *k = ' ';
            }
        }
    };
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        if c == '/' && i + 1 < n && bytes[i + 1] == '/' {
            let end = bytes[i..]
                .iter()
                .position(|&b| b == '\n')
                .map_or(n, |p| i + p);
            blank(&mut out, i, end);
            i = end;
        } else if c == '/' && i + 1 < n && bytes[i + 1] == '*' {
            let mut depth = 1usize;
            let mut j = i + 2;
            while j < n && depth > 0 {
                if j + 1 < n && bytes[j] == '/' && bytes[j + 1] == '*' {
                    depth += 1;
                    j += 2;
                } else if j + 1 < n && bytes[j] == '*' && bytes[j + 1] == '/' {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == '"' {
            let mut j = i + 1;
            while j < n {
                if bytes[j] == '\\' {
                    j += 2;
                } else if bytes[j] == '"' {
                    j += 1;
                    break;
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == 'r' || c == 'b' {
            let Some(open) = raw_string_open_quote(&bytes, i) else {
                i += 1;
                continue;
            };
            let prefix_end = if c == 'b' { i + 2 } else { i + 1 };
            let hashes = open - prefix_end;
            let close: Vec<char> = format!("\"{}", "#".repeat(hashes)).chars().collect();
            let mut end = n;
            let mut k = open + 1;
            while k + close.len() <= n {
                if bytes[k..k + close.len()] == close[..] {
                    end = k + close.len();
                    break;
                }
                k += 1;
            }
            blank(&mut out, i, end);
            i = end;
        } else if c == '\'' {
            let mut j = i + 1;
            if j < n && bytes[j] == '\\' {
                j += 1;
                if j < n && bytes[j] == '{' {
                    while j < n && bytes[j] != '}' {
                        j += 1;
                    }
                }
                j += 1;
            } else {
                j += 1;
            }
            if j < n && bytes[j] == '\'' {
                blank(&mut out, i, j + 1);
                i = j + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

pub fn matching_brace_end(masked: &str, open: usize) -> Option<usize> {
    if !masked[open..].starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (idx, ch) in masked[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + idx + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn mask_non_code_matches_shared_corpus() {
    let corpus = include_str!("../../../scripts/fixtures/rust-mask-corpus.rs");
    let expected = include_str!("../../../scripts/fixtures/rust-mask-corpus.expected.txt");
    assert_eq!(
        mask_non_code(corpus),
        expected,
        "掩码输出与共享语料期望漂移——规则改动必须与 TS 侧 maskNonCode \
         （scripts/gate-primitives.ts）同步并重新生成语料期望"
    );
}

#[test]
fn matching_brace_end_matches_nested_and_returns_past_close() {
    let masked = "fn f() { g({ a }); h(); } fn g() {}";
    let open = masked.find('{').unwrap();
    assert_eq!(
        matching_brace_end(masked, open),
        Some(masked.find("} fn").unwrap() + 1)
    );
}

#[test]
fn matching_brace_end_rejects_non_open_and_unbalanced() {
    assert_eq!(matching_brace_end("fn f() …", 0), None);
    let unbalanced = "fn f() { g();";
    let open = unbalanced.find('{').unwrap();
    assert_eq!(matching_brace_end(unbalanced, open), None);
}
