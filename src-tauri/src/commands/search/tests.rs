//! 搜索领域测试（issue #88 外迁，issue #196 重写）：统一模糊搜索语义纯函数
//! （拼音首字母/子序列判定/词条匹配/切词）与搜索行为（含金额/日期筛选、分页、
//! 排序、写入立即可搜）。

use rusqlite::Connection;

use super::query::search_transactions_internal;
use super::text::{is_subsequence, pinyin_initials, split_terms, term_matches, term_matches_text};
use crate::db::{init_db, open_in_memory};
use crate::error::Result;
use crate::models::TransactionSearchResult;

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
    // 备注 ∨ 转出账户名，任一命中即算
    assert!(term_matches("zsyh", Some("吃饭"), "招商银行"));
    assert!(term_matches("cf", Some("吃饭"), "招商银行"));
    assert!(term_matches("xj", None, "现金"));
    // 两字段皆不命中
    assert!(!term_matches("wy", Some("吃饭"), "招商银行"));
    // 备注为空时仅账户名路径
    assert!(!term_matches("cf", None, "现金"));
}

#[test]
fn split_terms_by_whitespace() {
    assert_eq!(split_terms("cf 午餐"), vec!["cf", "午餐"]);
    assert_eq!(split_terms("  多   词条  "), vec!["多", "词条"]);
    assert!(split_terms("").is_empty());
    assert!(split_terms("   ").is_empty());
    // 特殊字符按字面保留（无 FTS 语法，无需转义）
    assert_eq!(split_terms("午餐(1)"), vec!["午餐(1)"]);
}

// -----------------------------------------------------------------------
// 搜索行为
// -----------------------------------------------------------------------

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
    // 特殊字符按字面匹配，不再有 FTS 语法含义
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
    // 无索引：账户改名即时反映到搜索（原 FTS 实现需重建队列消费）
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
