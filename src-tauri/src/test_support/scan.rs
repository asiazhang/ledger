//! 源码扫描器具单一维护点（issue #1433）：Rust 侧源码扫描守门共用的词法掩码
//! 与花括号配对原语。
//!
//! 消费面：壳层信号守门（`signals_cross_check`）、连接槽守门（`db_slot_guard`）、
//! 同步触发守门（`sync_trigger_guard`）、行情接缝守门（`api_server::state`
//! 测试）、`commands/investment.rs` 扫描测试，以及行情同步域车道守门（经既有
//! `tauri-app` dev-dependency 环，spec #1086；登记处 ADR-0084 修订注记）。
//!
//! **双源登记**（#1433）：[`mask_non_code`] 与 TS 侧 `scripts/gate-primitives.ts`
//! 的 `maskNonCode` 是同一条 Rust 词法掩码规则的**两个运行时载体**（守门脚本跑
//! Bun、Rust 测试跑 cargo，单份实现不可共享），规则改动必须两侧同步；防漂移
//! 断言消费共享语料夹具 `scripts/fixtures/rust-mask-corpus.rs`（期望输出
//! `.expected.txt`），任一侧单独改规则即语料测试红。
//!
//! 已知双源边界（语料不含，#1433 交付报告登记）：非 ASCII 标识符紧邻原始串、
//! 非 BMP 字符入 char 字面量两形态，TS 侧按 ASCII/UTF-16 近似，两侧结果可能
//! 不同；`'\u{…}'` 转义两实现**一致地**不按字面量识别（两侧旧注释声称支持，
//! 与实现不符——#1433 改为如实描述），扩展属规则改动，须两侧同步 + 语料更新；
//! TS 侧另有 `keepLiterals` 扩展形态（保留字面量只掩注释，#1014），Rust 侧
//! 消费面无此需求未实现，属登记在案的不对称。

/// 若 `chars` 中 `i` 起是 Rust 原始字符串前缀，返回其后开引号下标；否则返回
/// `None`。覆盖 `r"…"` / `r#"…"` 与字节变体 `br"…"` / `br#"…"`，# 数任意；
/// 前一字符为标识符成分时是普通名字，不误伤。
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

/// 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白（保留换行
/// 与列位），使令牌扫描只落在真实代码上。处理形态：行注释（`//`、`///`、`//!`）、
/// 可嵌套块注释、普通字符串（含转义）、原始字符串 `r"…"` / `r#"…"` / `r##"…"`
/// 及其字节变体 `br"…"` / `br#"…"` / `br##"…"`（# 数任意；前一字符为标识符
/// 成分时是普通名字，不误伤）、char 字面量（`'a'`、`'\n'`、`'\\'`、`'\''`、
/// `'"'`）；生命周期标注（`'a`）按非字面量处理。`'\u{…}'` 不按字面量识别
///（见模块文档「双源登记」）。与 `maskNonCode`
///（`scripts/gate-primitives.ts`）双源同规，防漂移见模块文档。
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
            // 行注释（含 /// 与 //!）到行尾
            let end = bytes[i..]
                .iter()
                .position(|&b| b == '\n')
                .map_or(n, |p| i + p);
            blank(&mut out, i, end);
            i = end;
        } else if c == '/' && i + 1 < n && bytes[i + 1] == '*' {
            // 块注释（Rust 可嵌套）
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
            // 普通字符串：跳过转义对
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
            // 原始字符串 r"…" / r#"…" / r##"…" 与字节变体 br"…" / br#"…" / br##"…"
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
            // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
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

/// 掩码文本上从 `open`（必须指向 `{`）起做花括号配对，返回配对闭括号之后的
/// 字节偏移；`open` 不指向 `{` 或不配对（掩码文本不会发生，防御）时返回
/// `None`。注释与字符串已在掩码中空白化，花括号只来自真实代码，配对可靠。
/// #1433 上收：`signals_cross_check::fn_body_end`、`sync_trigger_guard::
/// production_text`、`commands/investment.rs` 命令体提取与 market-sync
/// `blank_inline_test_modules` 四处手写同型的单一实现。
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

/// 双源防漂移语料（#1433）：本实现与 TS 侧 `maskNonCode` 消费同一夹具，
/// 期望输出以 TS 侧权威实现生成；两侧单独改规则即本测或 vitest 侧语料测试红。
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
    // open 不指向 {
    assert_eq!(matching_brace_end("fn f() …", 0), None);
    // 不配对（掩码文本不会发生，防御路径）
    let unbalanced = "fn f() { g();";
    let open = unbalanced.find('{').unwrap();
    assert_eq!(matching_brace_end(unbalanced, open), None);
}
