r"语料首 token 落在字节 0：原始字符串的 i == 0 路径（前字符不存在，不构成标识符前缀）"
// 行注释（含 /// 与 //!）：write_entry( db::write( "quoted" r#"x"# 'a' 全部不计
/// 文档注释里的 "字符串" 与 /* 块注释样 */
//! 内部文档注释 thread::spawn block_on
/* 块注释基础形 "string" // line */
/* 块注释 /* 可嵌套 */ 仍是注释 "未闭合字符串 */ let code_after_block = 1;
let s = "字符串含 \\\" 转义与 // 非注释 与 /* 非块 */";
let empty = "";
let raw0 = r"raw with # inside";
let raw1 = r#"raw with "quote" and // not comment"#;
let url = r"http://example.com // not a comment";
let ident_like = 42;
timer"x" == "timer 字面量后随引号：r 是标识符尾字符，不构成原始串";
let chars = ['a', '\n', '\\', '\'', '\u{7FFF}', '"', '中'];
let lifetime_ref: &'a str = "生命周期 'a 不是 char 字面量";
let static_lt: &'static str = "'static 同上";
struct Point<'a> { label: &'a str }
#[cfg(test)]
mod corpus_tests {
    use super::*;
    #[test]
    fn mixed() {
        let sql = "SELECT 'a' FROM t // not comment";
        assert_eq!(sql, r#"SELECT 'a' FROM t // not comment"#);
    }
}
// 行注释收尾无换行符 "未闭合
