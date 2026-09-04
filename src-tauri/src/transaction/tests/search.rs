//! 搜索领域测试（issue #88 外迁，issue #196 重写）：统一模糊搜索语义纯函数
//! （拼音首字母/子序列判定/词条匹配/切词）与搜索行为（含金额/日期筛选、分页、
//! 排序、写入立即可搜）。

use std::collections::{BTreeSet, HashMap};

use rusqlite::Connection;

use super::super::search::{
    Stage1Filter, TermLowered, build_stage1_query, load_search_dicts, search_transactions_internal,
};
use crate::db::{init_db, open_in_memory};
use crate::error::Result;
use crate::transaction::TransactionSearchResult;
use crate::transaction::search_text::{
    is_subsequence, pinyin_initials, split_terms, term_matches, term_matches_text,
};
use crate::transaction::writer::{NormalizedRow, insert_row, update_row};

fn setup() -> Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    conn
}

/// 无筛选搜索（第 1 页、每页 20 条）。
fn search(conn: &Connection, query: &str) -> Result<TransactionSearchResult> {
    search_transactions_internal(conn, query, 1, 20, None, None, None, None)
}

/// 无筛选分页搜索。
fn search_paged(
    conn: &Connection,
    query: &str,
    page: usize,
    page_size: usize,
) -> Result<TransactionSearchResult> {
    search_transactions_internal(conn, query, page, page_size, None, None, None, None)
}

fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, name, kind, currency],
    )
    .unwrap();
}

fn insert_category(conn: &Connection, id: &str, name: &str, kind: &str) {
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,NULL,NULL,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, name, kind],
    )
    .unwrap();
}

fn insert_merchant(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, name],
    )
    .unwrap();
}

fn insert_txn(
    conn: &Connection,
    id: &str,
    account_id: &str,
    category_id: Option<&str>,
    note: Option<&str>,
    date: &str,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'expense',1000,'CNY',1000,?2,NULL,?3,NULL,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, account_id, category_id, note, date],
    )
    .unwrap();
}

/// 指定商户的交易（其余列与 `insert_txn` 一致）。
fn insert_txn_merchant(
    conn: &Connection,
    id: &str,
    account_id: &str,
    merchant_id: Option<&str>,
    note: Option<&str>,
    date: &str,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted,merchant_id) \
         VALUES (?1,'expense',1000,'CNY',1000,?2,NULL,NULL,NULL,?3,?4,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0,?5)",
        rusqlite::params![id, account_id, note, date, merchant_id],
    )
    .unwrap();
}

/// 指定金额的存量交易（其余列与 `insert_txn` 一致）。
fn insert_txn_amount(
    conn: &Connection,
    id: &str,
    account_id: &str,
    note: Option<&str>,
    date: &str,
    amount_cents: i64,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'expense',?2,'CNY',?2,?3,NULL,NULL,NULL,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, amount_cents, account_id, note, date],
    )
    .unwrap();
}

// -----------------------------------------------------------------------
// 拼音首字母
// -----------------------------------------------------------------------

#[test]
fn pinyin_initials_basic() {
    assert_eq!(pinyin_initials("招商银行"), "zsyh");
    assert_eq!(pinyin_initials("吃饭"), "cf");
    assert_eq!(pinyin_initials("餐饮"), "cy");
    assert_eq!(pinyin_initials("工资"), "gz");
}

#[test]
fn pinyin_initials_handles_mixed_and_ascii() {
    assert_eq!(pinyin_initials("ABC银行"), "abcyh");
    assert_eq!(pinyin_initials("无(CNY)"), "wcny");
    assert_eq!(pinyin_initials("12306"), "12306");
    assert_eq!(pinyin_initials(""), "");
    assert_eq!(pinyin_initials("---"), "");
}

#[test]
fn pinyin_initials_all_lowercase() {
    let out = pinyin_initials("招商银行");
    assert_eq!(out, out.to_lowercase());
}

#[test]
fn pinyin_initials_polyphone_bank() {
    // 多音字修正：银行/商业银行 → háng（h），而非默认行走 xíng（x）
    assert_eq!(pinyin_initials("银行"), "yh");
    assert_eq!(pinyin_initials("商业银行"), "syyh");
    // 非「银」前缀的「行」沿用默认读音
    assert_eq!(pinyin_initials("行走"), "xz");
}

// -----------------------------------------------------------------------
// 子序列判定
// -----------------------------------------------------------------------

#[test]
fn subsequence_empty_pattern_always_matches() {
    assert!(is_subsequence("", ""));
    assert!(is_subsequence("", "wkwy"));
}

#[test]
fn subsequence_empty_target_rejects_nonempty_pattern() {
    assert!(!is_subsequence("wy", ""));
}

#[test]
fn subsequence_contiguous_and_skipping() {
    // 不跳字（连续命中）与跳字（间隔命中）均可
    assert!(is_subsequence("wy", "wy"));
    assert!(is_subsequence("wy", "wkwy"));
    assert!(is_subsequence("zsyh", "zhaoshangyinhang"));
    // 顺序不可颠倒
    assert!(!is_subsequence("yw", "wkwy"));
}

#[test]
fn subsequence_repeated_chars_require_occurrences() {
    // 「万科物业」首字母 wkwy：两个 w 都有供给
    assert!(is_subsequence("wwy", "wkwy"));
    // 三个 w 只有两次供给，失败
    assert!(!is_subsequence("wwwy", "wkwy"));
    // pattern 长于 target 且字符相同时失败
    assert!(!is_subsequence("aa", "a"));
    assert!(is_subsequence("a", "a"));
}

#[test]
fn subsequence_case_insensitive_and_ascii() {
    assert!(is_subsequence("WY", "wkwy"));
    assert!(is_subsequence("wy", "WKWY"));
    assert!(is_subsequence("ABC", "a1b2c3"));
    assert!(is_subsequence("abc", "ABC"));
}

// -----------------------------------------------------------------------
// 统一语义匹配（词条 × 字段）
// -----------------------------------------------------------------------

#[test]
fn term_matches_by_contiguous_substring() {
    // 原文连续子串：大小写不敏感
    assert!(term_matches_text("万科", "万科物业"));
    assert!(term_matches_text("物业费", "万科物业费"));
    assert!(term_matches_text("ABC", "abc银行"));
    assert!(!term_matches_text("科物 万", "万科物业"));
}

#[test]
fn term_matches_by_initials_subsequence() {
    // 首字母子序列路径：wy 命中「万科物业」（wkwy）
    assert!(term_matches_text("wy", "万科物业"));
    assert!(term_matches_text("zsyh", "招商银行"));
    // 大小写不敏感
    assert!(term_matches_text("WY", "万科物业"));
    // 首字母路径不允许逆序
    assert!(!term_matches_text("yw", "万科物业"));
}

#[test]
fn term_matches_mixed_term_falls_back_to_substring() {
    // 含汉字的词条对纯 ASCII 首字母串的子序列匹配必然失败，
    // 自然落到原文子串路径：字面含「招zsyh」才命中
    assert!(term_matches_text("招zsyh", "旧账招zsyh导入"));
    assert!(!term_matches_text("招zsyh", "招商银行转账"));
}

#[test]
fn term_matches_ascii_literal_preserved() {
    // ASCII 原样保留：abcyh 命中「ABC银行」（首字母串 abcyh）
    assert!(term_matches_text("abcyh", "ABC银行"));
    assert!(term_matches_text("ABCYH", "ABC银行"));
    // 数字同样原样保留
    assert!(term_matches_text("123", "会员123"));
}

#[test]
fn term_matches_any_field() {
    // 备注 ∨ 转出账户名 ∨ 商户名，任一命中即算
    assert!(term_matches("zsyh", Some("吃饭"), "招商银行", Some("京东")));
    assert!(term_matches("cf", Some("吃饭"), "招商银行", Some("京东")));
    assert!(term_matches("xj", None, "现金", None));
    // 商户名路径：jd 命中「京东」
    assert!(term_matches("jd", None, "现金", Some("京东")));
    // 两字段皆不命中
    assert!(!term_matches("wy", Some("吃饭"), "招商银行", Some("京东")));
    // 备注为空时仅账户名/商户名路径
    assert!(!term_matches("cf", None, "现金", None));
}

#[test]
fn split_terms_by_whitespace() {
    assert_eq!(split_terms("cf 午餐"), vec!["cf", "午餐"]);
    assert_eq!(split_terms("  多   词条  "), vec!["多", "词条"]);
    assert!(split_terms("").is_empty());
    assert!(split_terms("   ").is_empty());
    // 特殊字符按字面保留（无查询语法，无需转义）
    assert_eq!(split_terms("午餐(1)"), vec!["午餐(1)"]);
}

// -----------------------------------------------------------------------
// 搜索行为
// -----------------------------------------------------------------------

#[test]
fn search_matches_merchant_name_and_initials() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_merchant(&conn, "m1", "京东");
    insert_merchant(&conn, "m2", "万科物业");
    insert_txn_merchant(&conn, "t1", "a1", Some("m1"), Some("购物"), "2026-02-01");
    insert_txn_merchant(&conn, "t2", "a1", Some("m2"), None, "2026-02-02");
    // 商户名原文子串（备注不命中）
    let res = search(&conn, "京东").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    // 商户名拼音首字母子序列：jd 命中「京东」，wkwy 命中「万科物业」（无索引，写入立即可搜）
    let res = search(&conn, "jd").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    let res = search(&conn, "wkwy").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t2");
    // 无商户交易不受影响：备注命中
    let res = search(&conn, "购物").unwrap();
    assert_eq!(res.total, 1);
    // 不存在的商户名不命中
    let res = search(&conn, "拼多多").unwrap();
    assert_eq!(res.total, 0);
}

#[test]
fn search_soft_deleted_merchant_still_searchable() {
    // 软删商户的历史交易仍按商户名可搜（与交易列表显示口径一致）
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_merchant(&conn, "m1", "京东");
    insert_txn_merchant(&conn, "t1", "a1", Some("m1"), None, "2026-02-01");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='m1'", [])
        .unwrap();
    let res = search(&conn, "京东").unwrap();
    assert_eq!(res.total, 1);
    let res = search(&conn, "jd").unwrap();
    assert_eq!(res.total, 1);
}

#[test]
fn merchant_rename_takes_effect_immediately() {
    // 无索引：商户改名即时反映到搜索（引用指向 id，按当前名字命中）
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_merchant(&conn, "m1", "京东");
    insert_txn_merchant(&conn, "t1", "a1", Some("m1"), None, "2026-02-01");
    conn.execute("UPDATE merchants SET name='物美超市' WHERE id='m1'", [])
        .unwrap();
    // 旧名不再命中
    let res = search(&conn, "京东").unwrap();
    assert_eq!(res.total, 0);
    // 新名与新名首字母即刻命中
    let res = search(&conn, "物美超市").unwrap();
    assert_eq!(res.total, 1);
    let res = search(&conn, "wmcs").unwrap();
    assert_eq!(res.total, 1);
}

#[test]
fn search_multi_term_combines_merchant_and_note() {
    // 词条 AND：商户名与备注分属不同词条，均命中才返回
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_merchant(&conn, "m1", "京东");
    insert_txn_merchant(&conn, "t1", "a1", Some("m1"), Some("键盘"), "2026-02-01");
    insert_txn_merchant(&conn, "t2", "a1", Some("m1"), Some("鼠标"), "2026-02-02");
    let res = search(&conn, "jd 键盘").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    let res = search(&conn, "jd 显示器").unwrap();
    assert_eq!(res.total, 0);
}

#[test]
fn search_matches_note_substring_and_account_initials() {
    let conn = setup();
    insert_account(&conn, "a1", "招商银行", "bank", "CNY");
    insert_account(&conn, "a2", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐外卖"), "2026-02-01");
    insert_txn(&conn, "t2", "a2", None, Some("打车"), "2026-02-02");
    // 备注原文子串
    let res = search(&conn, "外卖").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    // 账户名拼音首字母（无索引，写入立即可搜）
    let res = search(&conn, "zsyh").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    // 备注首字母子序列（午餐外卖 → wcwm，wm 为其子序列且非原文子串）
    let res = search(&conn, "wm").unwrap();
    assert_eq!(res.total, 1);
}

#[test]
fn search_multi_term_and_combination() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_account(&conn, "a2", "招商银行", "bank", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐"), "2026-02-01");
    insert_txn(&conn, "t2", "a2", None, Some("午餐"), "2026-02-02");
    // 词条 AND：两词条都命中的交易才返回
    let res = search(&conn, "午餐 现金").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    // 第二词条命中不同字段（账户名）
    let res = search(&conn, "午餐 zsyh").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t2");
    // 无交集
    let res = search(&conn, "午餐 晚餐").unwrap();
    assert_eq!(res.total, 0);
}

#[test]
fn search_case_insensitive_and_special_chars_literal() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("ATM(取款)"), "2026-02-01");
    // 大小写不敏感（原文子串路径）
    let res = search(&conn, "atm").unwrap();
    assert_eq!(res.total, 1);
    let res = search(&conn, "ATM").unwrap();
    assert_eq!(res.total, 1);
    // 特殊字符按字面匹配，不再有查询语法含义
    let res = search(&conn, "ATM(取款)").unwrap();
    assert_eq!(res.total, 1);
    let res = search(&conn, "午餐 AND 现金 OR (NOT)").unwrap();
    assert_eq!(res.total, 0);
}

#[test]
fn search_excludes_soft_deleted() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐"), "2026-02-01");
    insert_txn(&conn, "t2", "a1", None, Some("晚餐"), "2026-02-02");
    conn.execute("UPDATE transactions SET is_deleted=1 WHERE id='t1'", [])
        .unwrap();
    // 无索引：软删除即刻生效，无需任何刷新步骤
    let res = search(&conn, "午餐").unwrap();
    assert_eq!(res.total, 0);
    let res = search(&conn, "晚餐").unwrap();
    assert_eq!(res.total, 1);
}

#[test]
fn search_orders_by_date_desc() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐"), "2026-02-01");
    insert_txn(&conn, "t2", "a1", None, Some("午餐"), "2026-02-10");
    insert_txn(&conn, "t3", "a1", None, Some("午餐"), "2026-02-05");
    let res = search(&conn, "午餐").unwrap();
    assert_eq!(res.total, 3);
    // 固定交易日期降序（不再按相关度）
    let ids: Vec<&str> = res.items.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["t2", "t3", "t1"]);
}

#[test]
fn search_pagination_and_total() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    for (i, id) in ["t1", "t2", "t3", "t4", "t5"].iter().enumerate() {
        let date = format!("2026-01-{:02}", i + 1);
        insert_txn_amount(
            &conn,
            id,
            "a1",
            Some("午餐"),
            &date,
            1000 + (i as i64) * 100,
        );
    }
    let res = search_paged(&conn, "午餐", 1, 2).unwrap();
    assert_eq!(res.items.len(), 2);
    assert_eq!(res.total, 5);
    // 日期降序：第 1 页是最新两天
    assert_eq!(res.items[0].id, "t5");
    assert_eq!(res.items[1].id, "t4");
    let res = search_paged(&conn, "午餐", 3, 2).unwrap();
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].id, "t1");
    // 超出命中数的页返回空页，total 不变
    let res = search_paged(&conn, "午餐", 4, 2).unwrap();
    assert_eq!(res.items.len(), 0);
    assert_eq!(res.total, 5);
}

#[test]
fn search_extreme_page_inputs_do_not_panic() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐"), "2026-02-01");
    let res = search_paged(&conn, "午餐", usize::MAX, usize::MAX).unwrap();
    assert_eq!(res.total, 1);
    // offset 饱和钳制：超大页码返回空页而非 panic
    assert_eq!(res.items.len(), 0);
    let res = search_paged(&conn, "午餐", 0, 0).unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items.len(), 1);
}

#[test]
fn search_empty_query_returns_empty_without_filter() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐"), "2026-02-01");
    let res = search(&conn, "").unwrap();
    assert_eq!(res.total, 0);
    assert!(res.items.is_empty());
    let res = search(&conn, "   ").unwrap();
    assert_eq!(res.total, 0);
}

// -----------------------------------------------------------------------
// 金额/日期筛选（issue #40，与关键字 AND 组合）
// -----------------------------------------------------------------------

#[test]
fn search_amount_range_inclusive_bounds() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "t1", "a1", Some("早餐"), "2026-02-01", 1000);
    insert_txn_amount(&conn, "t2", "a1", Some("午餐"), "2026-02-02", 1550);
    insert_txn_amount(&conn, "t3", "a1", Some("晚餐"), "2026-02-03", 2000);
    let res =
        search_transactions_internal(&conn, "午餐", 1, 20, Some(1550), Some(2000), None, None)
            .unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t2");
}

#[test]
fn search_amount_filter_one_sided() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "t1", "a1", Some("早餐"), "2026-02-01", 1000);
    insert_txn_amount(&conn, "t2", "a1", Some("午餐"), "2026-02-02", 1500);
    insert_txn_amount(&conn, "t3", "a1", Some("晚餐"), "2026-02-03", 2000);
    let res = search_transactions_internal(&conn, "", 1, 20, Some(1500), None, None, None).unwrap();
    assert_eq!(res.total, 2);
    let res = search_transactions_internal(&conn, "", 1, 20, None, Some(1500), None, None).unwrap();
    assert_eq!(res.total, 2);
}

/// 金额区间过滤按本位币分（issue #395）：原始币种分与本位币分分叉时（外币 +
/// 汇率折算），过滤落在本位币列，与全仓聚合口径对齐，多币种下跨币种不再混滤
/// （此前按原始币种分过滤）。
#[test]
fn search_amount_range_filters_on_native_cents() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    // 外币交易：USD 100 元 = 本位币 720 元（amount_cents 与 amount_native_cents 分叉）
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('t1','expense',10000,'USD',72000,'a1',NULL,NULL,NULL,'美元订阅','2026-02-01',\
         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
    insert_txn_amount(&conn, "t2", "a1", Some("午餐"), "2026-02-02", 1500);
    // 本位币区间命中：72000 分落 [70000, 74000]（原始币种分 10000 不在此区间）
    let res = search_transactions_internal(&conn, "", 1, 20, Some(70000), Some(74000), None, None)
        .unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    // 原始币种分区间（10000 ±）不命中：过滤不再看原始币种列
    let res = search_transactions_internal(&conn, "", 1, 20, Some(9000), Some(11000), None, None)
        .unwrap();
    assert_eq!(res.total, 0);
    // 本位币交易不受影响：1500 分命中 [1500, 1500]
    let res =
        search_transactions_internal(&conn, "", 1, 20, Some(1500), Some(1500), None, None).unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t2");
}

#[test]
fn search_date_range_inclusive_bounds() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("早餐"), "2026-02-01");
    insert_txn(&conn, "t2", "a1", None, Some("午餐"), "2026-02-05");
    insert_txn(&conn, "t3", "a1", None, Some("晚餐"), "2026-02-10");
    let res = search_transactions_internal(
        &conn,
        "",
        1,
        20,
        None,
        None,
        Some("2026-02-01"),
        Some("2026-02-05"),
    )
    .unwrap();
    assert_eq!(res.total, 2);
    // 日期降序：午餐（02-05）在前
    assert_eq!(res.items[0].id, "t2");
    assert_eq!(res.items[1].id, "t1");
}

#[test]
fn search_filters_only_without_keyword() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "t1", "a1", Some("午餐"), "2026-02-01", 1500);
    insert_txn_amount(&conn, "t2", "a1", Some("晚餐"), "2026-02-02", 300);
    let res =
        search_transactions_internal(&conn, "", 1, 20, Some(1000), Some(2000), None, None).unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
}

#[test]
fn search_filters_exclude_soft_deleted_and_deleted_accounts() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_account(&conn, "a2", "已删账户", "cash", "CNY");
    insert_txn_amount(&conn, "t1", "a1", Some("午餐"), "2026-02-01", 1500);
    insert_txn_amount(&conn, "t2", "a2", Some("午餐"), "2026-02-02", 1500);
    conn.execute("UPDATE transactions SET is_deleted=1 WHERE id='t1'", [])
        .unwrap();
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='a2'", [])
        .unwrap();
    let res =
        search_transactions_internal(&conn, "", 1, 20, Some(1000), Some(2000), None, None).unwrap();
    assert_eq!(res.total, 0);
}

#[test]
fn search_includes_hidden_account_and_all_kinds() {
    let conn = setup();
    // 黑洞账户（type=other、隐藏）交易可搜，口径与交易列表一致
    insert_account(&conn, "a1", "无(CNY)", "other", "CNY");
    insert_account(&conn, "a2", "现金", "cash", "CNY");
    insert_account(&conn, "a3", "银行", "bank", "CNY");
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('t1','income',700,'CNY',700,'a1',NULL,NULL,NULL,'退款入账','2026-02-01','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
    insert_txn(&conn, "t2", "a2", None, Some("工资"), "2026-02-02");
    // 转账：转出账户名可搜
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('t3','transfer',3000,'CNY',3000,'a2','a3',NULL,NULL,NULL,'2026-02-03','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
    let res = search(&conn, "退款").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    let res = search(&conn, "工资").unwrap();
    assert_eq!(res.total, 1);
    // 转账交易按转出账户「现金」命中（转入账户名不在搜索范围，历史收窄语义保持）
    let res = search(&conn, "现金").unwrap();
    assert_eq!(res.total, 2);
    assert_eq!(res.items[0].id, "t3");
    let res = search(&conn, "银行").unwrap();
    assert_eq!(res.total, 0);
}

#[test]
fn account_rename_takes_effect_immediately() {
    // 无索引：账户改名即时反映到搜索
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("午餐"), "2026-02-01");
    let res = search(&conn, "zsyh").unwrap();
    assert_eq!(res.total, 0);
    conn.execute("UPDATE accounts SET name='招商银行' WHERE id='a1'", [])
        .unwrap();
    let res = search(&conn, "zsyh").unwrap();
    assert_eq!(res.total, 1);
}

#[test]
fn category_rename_does_not_affect_search() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_category(&conn, "c1", "餐饮", "expense");
    insert_txn(&conn, "t1", "a1", Some("c1"), Some("午餐"), "2026-02-01");
    // 分类名不在搜索范围：改名前后均不因分类名命中
    conn.execute("UPDATE categories SET name='吃喝' WHERE id='c1'", [])
        .unwrap();
    let res = search(&conn, "餐饮").unwrap();
    assert_eq!(res.total, 0);
    let res = search(&conn, "吃喝").unwrap();
    assert_eq!(res.total, 0);
}

// -----------------------------------------------------------------------
// 流式分页（见 ADR-0027 修订记录）：较大数据量下分页无重复、无遗漏、total 精确
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// V018 两段式取行：拼音冗余列 + 惰性回填（issue #492，语义与 ADR-0027 验收口径零变更）
// -----------------------------------------------------------------------

/// 指定备注拼音列的存量交易（其余列与 `insert_txn` 一致）：
/// note_pinyin = None 模拟 V018 之前的老行（升级后列为 NULL），Some(str) 模拟脏值。
fn insert_txn_note_pinyin(
    conn: &Connection,
    id: &str,
    account_id: &str,
    note: Option<&str>,
    date: &str,
    note_pinyin: Option<&str>,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,note_pinyin,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'expense',1000,'CNY',1000,?2,NULL,NULL,NULL,?3,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, account_id, note, note_pinyin, date],
    )
    .unwrap();
}

fn note_pinyin_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT note_pinyin FROM transactions WHERE id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

/// Writer 接缝维护冗余列：创建/修改随 note 同写同换，NULL note → NULL。
/// 搜索语义不受影响（该列只是匹配加速的派生数据）。
#[test]
fn writer_seam_populates_note_pinyin_on_insert_and_update() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    let row = NormalizedRow {
        kind: crate::transaction::TransactionKind::Expense,
        amount_cents: 1000,
        currency_code: "CNY".into(),
        amount_native_cents: 1000,
        account_id: "a1".into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: None,
        refund_of_transaction_id: None,
        note: Some("万科物业".into()),
        date: "2026-02-01".into(),
    };
    let id = insert_row(&conn, &row).unwrap();
    assert_eq!(note_pinyin_of(&conn, &id).as_deref(), Some("wkwy"));

    // 修改随新 note 重算。
    let mut new_row = row.clone();
    new_row.note = Some("招商银行".into());
    update_row(&conn, &id, &new_row).unwrap();
    assert_eq!(note_pinyin_of(&conn, &id).as_deref(), Some("zsyh"));

    // 清空备注 → 冗余列同步置 NULL。
    let mut empty_note = row.clone();
    empty_note.note = None;
    update_row(&conn, &id, &empty_note).unwrap();
    assert_eq!(note_pinyin_of(&conn, &id), None);
}

/// 惰性回填（issue #492）：存量老行（note_pinyin NULL）搜索即命中且语义不变，
/// 搜索后积压被分批回填；回填探针索引存在且收敛（命中集合不再变化）。
#[test]
fn lazy_backfill_heals_legacy_rows_on_search() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    // V018 之前的存量行形态：note 有值、拼音列为 NULL。
    insert_txn_note_pinyin(&conn, "t1", "a1", Some("万科物业"), "2026-02-01", None);
    insert_txn_note_pinyin(&conn, "t2", "a1", Some("招商银行转账"), "2026-02-02", None);
    // 回填探针索引存在（惰性回填的 O(1) 探测基础）。
    let backlog_index: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' \
             AND name='idx_transactions_note_pinyin_backlog'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(backlog_index, 1, "V018 应创建回填探针 partial 索引");

    // 搜索即回填（回填先于下推查询，issue #515 起不再运行时现算兑底）：
    // 拼音子序列语义立即生效，不因列缺失而漏匹配。
    let res = search(&conn, "wy").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    let res = search(&conn, "zsyh").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t2");

    // 搜索后：积压已被惰性回填，冗余列与现算规则一致。
    assert_eq!(note_pinyin_of(&conn, "t1").as_deref(), Some("wkwy"));
    assert_eq!(note_pinyin_of(&conn, "t2").as_deref(), Some("zsyhzz"));

    // 回填后语义不变（同一查询命中集合一致）。
    let res = search(&conn, "wy").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
}

/// 惰性回填兜底：手工脏值（拼音列与 note 不一致的行）不阻断匹配——原文子串
/// 路径始终按 note 现判，拼音子序列路径按列判（派生列允许漂移，审计不在此）。
#[test]
fn search_uses_pinyin_column_for_subsequence_path() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    // 列已回填：拼音子序列走列值（不逐行重算）。
    insert_txn_note_pinyin(
        &conn,
        "t1",
        "a1",
        Some("万科物业"),
        "2026-02-01",
        Some("wkwy"),
    );
    let res = search(&conn, "wy").unwrap();
    assert_eq!(res.total, 1);
    // 原文子串路径不受列值影响。
    let res = search(&conn, "物业").unwrap();
    assert_eq!(res.total, 1);
}

/// 第一段下推查询的计划钉定（父 #489 用户故事 18 / issue #515 验收）：下推查询
/// 必须命中 V018 搜索覆盖索引（idx_transactions_note_search，COVERING INDEX）
/// 且不产生 ORDER BY 临时 B-tree——planner 漂移在 CI 即刻暴露。
#[test]
fn stage1_scan_plan_uses_list_order_index() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_merchant(&conn, "m1", "京东");
    let dicts = load_search_dicts(&conn).unwrap();
    let term_lowers: Vec<TermLowered> = split_terms("kf jd")
        .iter()
        .map(|t| TermLowered {
            lower: t.to_lowercase(),
        })
        .collect();
    // 纯关键字与关键字 + 金额/日期筛选两种形态都必须钉定覆盖索引、无临时 B-tree。
    let filter_sets: [Vec<Stage1Filter>; 2] = [
        Vec::new(),
        vec![
            Stage1Filter {
                column: "t.amount_native_cents",
                op: ">=",
                value: rusqlite::types::Value::Integer(1_000),
            },
            Stage1Filter {
                column: "t.date",
                op: "<=",
                value: rusqlite::types::Value::Text("2026-12-31".into()),
            },
        ],
    ];
    for filters in &filter_sets {
        let query = build_stage1_query(&term_lowers, &dicts, filters);
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", query.sql))
            .unwrap();
        let details: Vec<String> = stmt
            .query_map(rusqlite::params_from_iter(query.params.iter()), |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        let plan = details.join(" | ");
        assert!(
            plan.contains("idx_transactions_note_search"),
            "下推查询应命中搜索覆盖索引: {plan}"
        );
        if filters.is_empty() {
            // 纯关键字：无约束可用，应为覆盖索引全扫（index-only 零回表）。
            assert!(
                plan.contains("SCAN t USING COVERING INDEX idx_transactions_note_search"),
                "纯关键字下推应为覆盖索引全扫: {plan}"
            );
        }
        assert!(
            !plan.to_uppercase().contains("TEMP B-TREE"),
            "排序应由索引序满足，不应出现临时 B-tree: {plan}"
        );
    }
}

#[test]
fn streaming_pagination_no_dup_no_gap_and_exact_total() {
    // 流式实现（游标逐行、命中即计数、仅当前页物化）下，逐页遍历所有命中
    // 应恰好覆盖全部命中（无重复、无遗漏），且 total 与逐页累加一致。
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    // 批量事务插入，避免逐条 autocommit 拖慢测试。
    {
        let tx = conn.unchecked_transaction().unwrap();
        for i in 0..500 {
            let date = format!("2026-{:02}-{:02}", 1 + (i / 28), 1 + (i % 28));
            insert_txn(&tx, &format!("t{i:03}"), "a1", None, Some("午餐"), &date);
        }
        tx.commit().unwrap();
    }
    const PAGE_SIZE: usize = 47; // 非整除页大小，逼出末页不足与跨页边界。
    let mut seen: Vec<String> = Vec::new();
    let mut page = 1;
    loop {
        let res = search_paged(&conn, "午餐", page, PAGE_SIZE).unwrap();
        for item in &res.items {
            seen.push(item.id.clone());
        }
        if page * PAGE_SIZE >= res.total as usize {
            break;
        }
        page += 1;
    }
    // 无重复、无遗漏：共 500 笔命中，且 id 集合唯一。
    assert_eq!(seen.len(), 500);
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 500);
    // 与单次全量 total 一致。
    let all = search_paged(&conn, "午餐", 1, 200).unwrap();
    assert_eq!(all.total, 500);
}

// -----------------------------------------------------------------------
// SQL 下推（issue #515，修订 ADR-0027）：转义字面、已知边界与「不漏」等价性
// -----------------------------------------------------------------------

/// LIKE 通配符按字面匹配（issue #515 验收）：`%`、`_`、`\` 经 ESCAPE 转义后
/// 按字面命中——搜「100%」不误命中任意后缀，`_` 不当单字通配，`\` 自身可搜。
#[test]
fn search_like_wildcards_match_literally() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("完成100%"), "2026-02-01");
    // 误命中哨兵：若 `%`/`_` 未转义，这些行会被通配误命中。
    insert_txn(&conn, "t2", "a1", None, Some("完成100x"), "2026-02-02");
    insert_txn(&conn, "t3", "a1", None, Some("a_b 测试"), "2026-02-03");
    insert_txn(&conn, "t4", "a1", None, Some("axb 测试"), "2026-02-04");
    insert_txn(&conn, "t5", "a1", None, Some("路径C:\\Users"), "2026-02-05");
    // 「100%」字面命中，不通配到「100x」。
    let res = search(&conn, "100%").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t1");
    // 「_」字面命中，不通配到「axb」。
    let res = search(&conn, "a_b").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t3");
    // 「\」自身按字面可搜（转义符翻倍）。
    let res = search(&conn, "C:\\Users").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t5");
}

/// Unicode 非 ASCII 大小写折叠是显式已知边界（issue #515 / ADR-0027 修订）：
/// SQLite LIKE 大小写折叠仅对 ASCII 生效，备注含非 ASCII 大写字母（É）且用户
/// 以另一大小写搜索时不命中（ASCII 部分与中文子串不受影响；账户/商户名字典
/// 路径仍在 Rust 侧全 Unicode 折叠，不受此边界影响）。
#[test]
fn unicode_non_ascii_case_folding_is_known_boundary() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn(&conn, "t1", "a1", None, Some("CAFÉ 午餐"), "2026-02-01");
    // 已知边界：非 ASCII 大写 É 不折叠到 é。
    let res = search(&conn, "café").unwrap();
    assert_eq!(res.total, 0);
    // ASCII 折叠不受影响（大写备注搜小写）。
    insert_txn(&conn, "t2", "a1", None, Some("ATM 转账"), "2026-02-02");
    let res = search(&conn, "atm").unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].id, "t2");
    // 中文子串不受影响。
    let res = search(&conn, "午餐").unwrap();
    assert_eq!(res.total, 1);
}

/// 「不漏」等价性回归网（issue #515 验收）：以纯函数重算旧 Rust 逐行语义的
/// 期望命中集合，断言 SQL 下推命中集合与之一致（⊇ 不遗漏且无误命中），覆盖
/// 中文子串/拼音子序列/混合输入/多词条 AND/ASCII 大小写/数字/标点字面/
/// 软删口径/零命中/高命中。
#[test]
fn pushdown_hit_set_covers_row_semantics() {
    let conn = setup();
    // 字典夹具：id → (名字, 是否软删) / id → 名字。
    let accounts: Vec<(&str, &str, bool)> = vec![
        ("a1", "现金", false),
        ("a2", "招商银行", false),
        ("a3", "已删咖啡账户", true),
        ("a4", "咖啡厅账户", false),
    ];
    let merchants: Vec<(&str, &str, bool)> =
        vec![("m1", "京东", false), ("m2", "已删外卖商户", true)];
    let categories: Vec<(&str, bool)> = vec![("c1", false), ("c9", true)];
    for (id, name, deleted) in &accounts {
        insert_account(&conn, id, name, "cash", "CNY");
        if *deleted {
            conn.execute("UPDATE accounts SET is_deleted=1 WHERE id=?1", [id])
                .unwrap();
        }
    }
    for (id, name, deleted) in &merchants {
        insert_merchant(&conn, id, name);
        if *deleted {
            conn.execute("UPDATE merchants SET is_deleted=1 WHERE id=?1", [id])
                .unwrap();
        }
    }
    for (id, deleted) in &categories {
        insert_category(
            &conn,
            id,
            if *deleted { "已删分类" } else { "餐饮" },
            "expense",
        );
        if *deleted {
            conn.execute("UPDATE categories SET is_deleted=1 WHERE id=?1", [id])
                .unwrap();
        }
    }

    /// 测试夹具中的一笔交易（期望侧重算与落库同源）。
    struct FixtureTxn {
        id: String,
        note: Option<&'static str>,
        account_id: &'static str,
        merchant_id: Option<&'static str>,
        category_id: Option<&'static str>,
        deleted: bool,
    }
    let mut txns: Vec<FixtureTxn> = (0..40)
        .map(|i| FixtureTxn {
            id: format!("r{i:02}"),
            note: Some("普通记录"),
            account_id: "a1",
            merchant_id: None,
            category_id: None,
            deleted: false,
        })
        .collect();
    txns.push(FixtureTxn {
        id: "t_cafe".into(),
        note: Some("买咖啡"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_kfc".into(),
        note: Some("kfc炸鸡"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_wkwy".into(),
        note: Some("万科物业费"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_zsyh_row".into(),
        note: None,
        account_id: "a2",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_jd_row".into(),
        note: Some("键盘"),
        account_id: "a1",
        merchant_id: Some("m1"),
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_delm_row".into(),
        note: Some("外卖历史单"),
        account_id: "a1",
        merchant_id: Some("m2"),
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_mixed".into(),
        note: Some("旧账zsyh导入"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_abc".into(),
        note: Some("ABC超市"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_num".into(),
        note: Some("会员123"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_pct".into(),
        note: Some("完成100%"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_und".into(),
        note: Some("a_b测试"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_bs".into(),
        note: Some("x\\y路径"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_both".into(),
        note: Some("咖啡报销"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_accdel".into(),
        note: Some("咖啡"),
        account_id: "a3",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_catdel".into(),
        note: Some("咖啡"),
        account_id: "a1",
        merchant_id: None,
        category_id: Some("c9"),
        deleted: false,
    });
    txns.push(FixtureTxn {
        id: "t_txndel".into(),
        note: Some("咖啡"),
        account_id: "a1",
        merchant_id: None,
        category_id: None,
        deleted: true,
    });
    txns.push(FixtureTxn {
        id: "t_kfacct".into(),
        note: Some("聚餐"),
        account_id: "a4",
        merchant_id: None,
        category_id: None,
        deleted: false,
    });
    {
        let dbtx = conn.unchecked_transaction().unwrap();
        for (i, t) in txns.iter().enumerate() {
            let date = format!("2026-01-{:02}", 1 + i % 28);
            insert_txn_merchant(&dbtx, &t.id, t.account_id, t.merchant_id, t.note, &date);
            if let Some(cid) = t.category_id {
                dbtx.execute(
                    "UPDATE transactions SET category_id=?1 WHERE id=?2",
                    rusqlite::params![cid, t.id],
                )
                .unwrap();
            }
            if t.deleted {
                dbtx.execute("UPDATE transactions SET is_deleted=1 WHERE id=?1", [&t.id])
                    .unwrap();
            }
        }
        dbtx.commit().unwrap();
    }

    // 期望侧：旧 Rust 逐行语义重算（词条 AND、字段 OR、口径同搜索）。
    // 拼音串取 pinyin_initials(note)，与 Writer/回填同规则（列值同源）。
    let account_by_id: HashMap<&str, (&str, bool)> =
        accounts.iter().map(|(id, n, d)| (*id, (*n, *d))).collect();
    let merchant_by_id: HashMap<&str, &str> =
        merchants.iter().map(|(id, n, _)| (*id, *n)).collect();
    let category_deleted: HashMap<&str, bool> =
        categories.iter().map(|(id, d)| (*id, *d)).collect();
    let expected_hits = |terms: &[&str]| -> BTreeSet<String> {
        txns.iter()
            .filter(|t| {
                if t.deleted {
                    return false;
                }
                let Some((name, deleted)) = account_by_id.get(t.account_id) else {
                    return false;
                };
                if *deleted {
                    return false;
                }
                if let Some(cid) = t.category_id
                    && category_deleted.get(cid).copied().unwrap_or(false)
                {
                    return false;
                }
                let merchant_name = t.merchant_id.and_then(|m| merchant_by_id.get(m).copied());
                terms
                    .iter()
                    .all(|term| term_matches(term, t.note, name, merchant_name))
            })
            .map(|t| t.id.to_string())
            .collect()
    };
    let actual_hits = |query: &str| -> BTreeSet<String> {
        let res = search_paged(&conn, query, 1, 200).unwrap();
        res.items.iter().map(|t| t.id.clone()).collect()
    };
    let matrix: Vec<(&str, Vec<&str>)> = vec![
        ("中文子串", vec!["咖啡"]),
        ("拼音子序列", vec!["kf"]),
        ("拼音子序列长串", vec!["wkwy"]),
        ("账户名拼音", vec!["zsyh"]),
        ("商户名原文", vec!["京东"]),
        ("软删商户名仍可搜", vec!["已删外卖商户"]),
        ("混合输入", vec!["招zsyh"]),
        ("多词条AND", vec!["咖啡", "报销"]),
        ("ASCII大写", vec!["ABC"]),
        ("ASCII小写", vec!["abc"]),
        ("数字", vec!["123"]),
        ("标点百分号", vec!["100%"]),
        ("标点下划线", vec!["a_b"]),
        ("标点反斜杠", vec!["x\\y"]),
        ("零命中", vec!["不存在的词条xyz"]),
        ("高命中", vec!["记录"]),
        ("高命中加词条", vec!["记录", "普通"]),
    ];
    for (label, terms) in matrix {
        let expected = expected_hits(&terms);
        let actual = actual_hits(&terms.join(" "));
        assert!(
            expected.is_subset(&actual),
            "「{label}」SQL 下推命中集合必须覆盖旧实现（不漏）: 缺失 {:?}",
            expected.difference(&actual).collect::<Vec<_>>()
        );
        assert_eq!(
            expected.len(),
            actual.len(),
            "「{label}」命中数应与旧实现一致（无误命中）"
        );
    }
}
