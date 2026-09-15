//! JS 文本字面量提取原语（[`super::fund_nav`] 与 [`super::bulk`] 消费）：东财多条
//! 通道的响应是 `.js` 数据文件或 `var x = …` 形态的非 JSON 文本，`=` 与 `:` 两种
//! 赋值形态都取自声明处。
//!
//! 不变量：只回答「声明的字面量正文是什么」，不回答「值可不可信」——可信度是各
//! 通道解析层的 fail-closed 判据。被拦截形态（风控 HTML 页、无权限报文）天然缺
//! 声明，提取失败即由调用方按不可信数据处置。

/// 从 JS 文本里取出 `var <marker> = "..."` 的字符串字面量（基金名称与代码的实际
/// 形态不含转义，不做转义处理）。未声明 / 缺 `=` / 缺引号均返回 None。
pub(super) fn declared_string<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let after_marker = &text[text.find(marker)? + marker.len()..];
    let after_eq = &after_marker[after_marker.find('=')? + 1..];
    let open = after_eq.find('"')?;
    let rest = &after_eq[open + 1..];
    let close = rest.find('"')?;
    Some(&rest[..close])
}

/// 从 JS 文本里取出 `marker` 之后的**第一个**数组字面量：先按 `marker` 定位（变量名
/// 或对象字段名，如 `var r` / `Data_netWorthTrend` / `datas`——`=` 与 `:` 两种赋值形态
/// 都适用），再从其后第一个 `[` 做括号配对（跳过 JSON 字符串内的括号与转义）。
/// 未声明、缺 `[` 或括号不闭合均返回 None（调用方按不可信数据 fail-closed）。
pub(super) fn declared_array<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let after_marker = &text[text.find(marker)? + marker.len()..];
    let open = after_marker.find('[')?;
    let bytes = after_marker.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate().skip(open) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&after_marker[open..=index]);
                }
            }
            _ => {}
        }
    }
    None
}
