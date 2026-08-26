//! 搜索领域测试（issue #88 外迁）：拼音首字母、可搜索内容组装、查询构建、
//! 搜索行为（含金额/日期筛选）、索引维护与后台刷新语义。

use rusqlite::Connection;

use super::index::{process_reindex_queue, rebuild_search_index, reconcile_search_index};
use super::query::search_transactions_internal;
use super::text::{build_match_query, build_search_content, pinyin_initials};
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

// -----------------------------------------------------------------------
// 可搜索内容组装
// -----------------------------------------------------------------------

#[test]
fn build_content_joins_note_account_and_initials() {
    let content = build_search_content(Some("吃饭"), "招商银行");
    assert_eq!(content, "吃饭 招商银行 cf zsyh");
}

#[test]
fn build_content_skips_empty_fields() {
    assert_eq!(build_search_content(None, "现金"), "现金 xj");
    assert_eq!(build_search_content(Some("   "), "现金"), "现金 xj");
    assert_eq!(build_search_content(None, ""), "");
}

// -----------------------------------------------------------------------
// 查询构建
// -----------------------------------------------------------------------

#[test]
fn build_match_query_single_term_with_prefix() {
    assert_eq!(build_match_query("午餐"), "(\"午餐\" OR \"午餐\"*)");
    assert_eq!(build_match_query("cf"), "(\"cf\" OR \"cf\"*)");
}

#[test]
fn build_match_query_multi_terms_and_joined() {
    assert_eq!(
        build_match_query("cf 午餐"),
        "(\"cf\" OR \"cf\"*) AND (\"午餐\" OR \"午餐\"*)"
    );
}

#[test]
fn build_match_query_escapes_special_chars() {
    // AND/OR/NOT 被引号包裹后成为字面量
    assert_eq!(
        build_match_query("午餐 AND 晚餐"),
        "(\"午餐\" OR \"午餐\"*) AND (\"AND\" OR \"AND\"*) AND (\"晚餐\" OR \"晚餐\"*)"
    );
    // 引号与星号剥离
    assert_eq!(build_match_query("a\"b*c"), "(\"abc\" OR \"abc\"*)");
    // 括号等保留在引号内
    assert_eq!(build_match_query("(abc)"), "(\"(abc)\" OR \"(abc)\"*)");
}

#[test]
fn build_match_query_empty_and_whitespace() {
    assert_eq!(build_match_query(""), "");
    assert_eq!(build_match_query("   "), "");
    assert_eq!(build_match_query("\"\"\"\"**"), "");
}

// -----------------------------------------------------------------------
// 搜索行为
// -----------------------------------------------------------------------

#[test]
fn search_matches_note_account_and_pinyin() {
    let conn = setup();
    insert_account(&conn, "acc-1", "招商银行", "bank", "CNY");
    insert_account(&conn, "acc-2", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("吃饭"), "2026-02-01");
    insert_txn(&conn, "tx-2", "acc-2", None, None, "2026-02-02");
    rebuild_search_index(&conn).unwrap();

    // 备注整词
    let r = search(&conn, "吃饭").unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items[0].id, "tx-1");
    // 拼音首字母 cf
    assert_eq!(search(&conn, "cf").unwrap().total, 1);
    // 账户名整词
    assert_eq!(search(&conn, "招商").unwrap().total, 1);
    // 账户名拼音 zsyh
    assert_eq!(search(&conn, "zsyh").unwrap().total, 1);
    // 前缀通配：吃 → 吃饭
    assert_eq!(search(&conn, "吃").unwrap().total, 1);
    // 整词不命中子串
    assert_eq!(search(&conn, "商银").unwrap().total, 0);
}

#[test]
fn search_multi_keyword_and_combination() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    insert_txn(&conn, "tx-2", "acc-1", None, Some("晚餐"), "2026-02-02");
    rebuild_search_index(&conn).unwrap();

    // 两个词条同时命中才返回（AND 语义）
    let r = search(&conn, "午餐 现金").unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items[0].id, "tx-1");
    let r = search(&conn, "午餐 晚餐").unwrap();
    assert_eq!(r.total, 0);
}

#[test]
fn search_excludes_soft_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    rebuild_search_index(&conn).unwrap();
    assert_eq!(search(&conn, "午餐").unwrap().total, 1);

    // 软删除后索引文档被移除，搜索结果消失
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at='2026-02-03T00:00:00Z', version=version+1 WHERE id='tx-1'",
        [],
    )
    .unwrap();
    process_reindex_queue(&conn).unwrap();
    assert_eq!(search(&conn, "午餐").unwrap().total, 0);
}

#[test]
fn search_rank_first_then_date_desc() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    // tx-a：命中词条更多、相关度更高，但日期更早
    insert_txn(
        &conn,
        "tx-a",
        "acc-1",
        None,
        Some("午餐 晚餐 早餐"),
        "2026-01-01",
    );
    // tx-b：命中词条更少、相关度更低，但日期更新
    insert_txn(&conn, "tx-b", "acc-1", None, Some("午餐"), "2026-02-01");
    rebuild_search_index(&conn).unwrap();

    let r = search(&conn, "午餐").unwrap();
    assert_eq!(r.items[0].id, "tx-a", "相关度 rank 优先于日期倒序");
    assert_eq!(r.items[1].id, "tx-b");
}

#[test]
fn search_pagination_and_total() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    for i in 1..=5 {
        insert_txn(
            &conn,
            &format!("tx-{i}"),
            "acc-1",
            None,
            Some("午餐"),
            &format!("2026-01-{i:02}"),
        );
    }
    rebuild_search_index(&conn).unwrap();

    let r = search_paged(&conn, "午餐", 1, 2).unwrap();
    assert_eq!(r.total, 5);
    assert_eq!(r.items.len(), 2);
    assert_eq!(r.items[0].id, "tx-5", "日期倒序第一页首条应为最新");

    let r = search_paged(&conn, "午餐", 3, 2).unwrap();
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].id, "tx-1");
}

#[test]
fn search_transfer_by_account_name() {
    use crate::commands::transactions::insert_transaction;
    use crate::models::TransactionInput;
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_account(&conn, "acc-2", "招商银行", "bank", "CNY");
    let input = TransactionInput {
        kind: "transfer".into(),
        amount_cents: 3000,
        currency_code: "CNY".into(),
        account_id: "acc-1".into(),
        to_account_id: Some("acc-2".into()),
        category_id: None,
        refund_of_transaction_id: None,
        note: Some("转账".into()),
        date: "2026-02-01".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let id = insert_transaction(&conn, input).unwrap();
    // 写入路径不做同步索引（ADR-0004 决策 #14）：消费队列后转出账户名
    // （含拼音首字母）可搜；转入账户名不在索引中
    process_reindex_queue(&conn).unwrap();
    assert_eq!(search(&conn, "现金").unwrap().total, 1);
    assert_eq!(search(&conn, "xj").unwrap().total, 1);
    assert_eq!(search(&conn, "招商").unwrap().total, 0);
    let _ = id;
}

#[test]
fn search_extreme_page_inputs_do_not_panic() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    rebuild_search_index(&conn).unwrap();

    // 极端输入：usize::MAX 页/页大小不 panic、不破坏 total；page=0 钳制为 1
    let r = search_paged(&conn, "午餐", usize::MAX, usize::MAX).unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items.len(), 0);
    let r = search_paged(&conn, "午餐", 0, 0).unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items.len(), 1);
}

#[test]
fn search_empty_query_and_special_chars() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    rebuild_search_index(&conn).unwrap();

    assert_eq!(search(&conn, "").unwrap().total, 0);
    assert_eq!(search(&conn, "   ").unwrap().total, 0);
    // 特殊字符不报错、不误命中
    let r = search(&conn, "午餐 AND 现金 OR (NOT)").unwrap();
    assert_eq!(r.total, 0);
    let r = search(&conn, "午餐\"").unwrap();
    assert_eq!(r.total, 1, "剥离引号后仍命中");
}

// -----------------------------------------------------------------------
// 金额/日期筛选（issue #40）
// -----------------------------------------------------------------------

#[test]
fn search_amount_range_inclusive_bounds() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1550);
    insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 2000);
    insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-03", 3000);
    rebuild_search_index(&conn).unwrap();

    // 区间含边界：1550 与 2000 都应命中，3000 不命中
    let r =
        search_transactions_internal(&conn, "", 1, 20, Some(1550), Some(2000), None, None).unwrap();
    assert_eq!(r.total, 2);
    assert_eq!(r.items[0].id, "tx-2", "无关键字时按日期倒序");
    assert_eq!(r.items[1].id, "tx-1");
}

#[test]
fn search_amount_filter_one_sided() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1000);
    insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 1500);
    insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-03", 2000);
    rebuild_search_index(&conn).unwrap();

    // 只填下限（含边界）
    let r = search_transactions_internal(&conn, "", 1, 20, Some(1500), None, None, None).unwrap();
    assert_eq!(r.total, 2, "金额下限含边界：1500、2000");
    // 只填上限（含边界）
    let r = search_transactions_internal(&conn, "", 1, 20, None, Some(1500), None, None).unwrap();
    assert_eq!(r.total, 2, "金额上限含边界：1000、1500");
}

#[test]
fn search_date_range_inclusive_bounds() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1000);
    insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-05", 1000);
    insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-10", 1000);
    rebuild_search_index(&conn).unwrap();

    // 日期区间含边界：02-01 与 02-05 命中，02-10 不命中
    let r = search_transactions_internal(
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
    assert_eq!(r.total, 2);
    // 单边日期
    let r = search_transactions_internal(&conn, "", 1, 20, None, None, Some("2026-02-05"), None)
        .unwrap();
    assert_eq!(r.total, 2, "起始日期含边界：02-05、02-10");
    let r = search_transactions_internal(&conn, "", 1, 20, None, None, None, Some("2026-02-05"))
        .unwrap();
    assert_eq!(r.total, 2, "结束日期含边界：02-01、02-05");
}

#[test]
fn search_filters_combined_with_keyword_and() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    // 命中关键字 + 金额区间 + 日期区间
    insert_txn_amount(&conn, "tx-1", "acc-1", Some("午餐"), "2026-02-01", 1550);
    // 金额超区间
    insert_txn_amount(&conn, "tx-2", "acc-1", Some("午餐"), "2026-02-02", 3000);
    // 日期超区间
    insert_txn_amount(&conn, "tx-3", "acc-1", Some("午餐"), "2026-02-10", 1550);
    // 金额、日期均命中但无关键字
    insert_txn_amount(&conn, "tx-4", "acc-1", None, "2026-02-03", 1550);
    rebuild_search_index(&conn).unwrap();

    let r = search_transactions_internal(
        &conn,
        "午餐",
        1,
        20,
        Some(1550),
        Some(2000),
        Some("2026-02-01"),
        Some("2026-02-05"),
    )
    .unwrap();
    assert_eq!(r.total, 1, "关键字与金额/日期筛选 AND 组合");
    assert_eq!(r.items[0].id, "tx-1");
}

#[test]
fn search_filters_only_without_keyword() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "tx-1", "acc-1", Some("午餐"), "2026-02-01", 1550);
    insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 3000);
    rebuild_search_index(&conn).unwrap();

    // 空查询 + 有筛选 → 执行仅筛选查询（放开空查询直返空）
    let r =
        search_transactions_internal(&conn, "   ", 1, 20, Some(2000), None, None, None).unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(r.items[0].id, "tx-2");
    // 空查询 + 无筛选 → 维持空结果
    assert_eq!(search(&conn, "").unwrap().total, 0);
    assert_eq!(search(&conn, "   ").unwrap().total, 0);
}

// -----------------------------------------------------------------------
// 后台定时刷新（ADR-0004 决策 #14）与 stale 标志
// -----------------------------------------------------------------------

#[test]
fn write_path_does_not_index_until_queue_consumed() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    // 单笔写入路径不再同步建索引（触发器已入队）：未消费前搜不到
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    assert_eq!(
        search(&conn, "午餐").unwrap().total,
        0,
        "写入后未刷新不可搜"
    );
    // 消费队列后立即可搜
    process_reindex_queue(&conn).unwrap();
    assert_eq!(search(&conn, "午餐").unwrap().total, 1);
}

#[test]
fn search_reports_stale_while_queue_pending() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    rebuild_search_index(&conn).unwrap();
    assert!(!search(&conn, "午餐").unwrap().stale, "队列为空时不滞后");

    // 软删除入队（触发器）后队列非空：搜索报告 stale=true（搜索不消费队列）
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    let r = search(&conn, "午餐").unwrap();
    assert!(r.stale, "存在未消费写入时 stale=true");

    // 消费后队列清空：stale 回落 false
    process_reindex_queue(&conn).unwrap();
    assert!(!search(&conn, "午餐").unwrap().stale);
}

#[test]
fn batch_import_consumes_queue_immediately() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    let input = crate::models::TransactionInput {
        kind: "expense".into(),
        amount_cents: 1000,
        currency_code: "CNY".into(),
        account_id: "acc-1".into(),
        to_account_id: None,
        category_id: None,
        refund_of_transaction_id: None,
        note: Some("午餐".into()),
        date: "2026-02-01".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    // 批量编排模块内部在事务提交后立即消费队列（直调 TransactionBatch::run，issue #65）
    crate::commands::batch::TransactionBatch::run(&conn, vec![input], false).unwrap();
    assert_eq!(search(&conn, "午餐").unwrap().total, 1, "导入后立即可搜");
    assert!(!search(&conn, "午餐").unwrap().stale, "导入消费后不滞后");
}

#[test]
fn search_amount_and_date_filters_without_keyword() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1000);
    insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 1550);
    insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-10", 1550);
    rebuild_search_index(&conn).unwrap();

    // 无关键字 + 金额与日期同时筛选（AND 组合，含边界）
    let r = search_transactions_internal(
        &conn,
        "",
        1,
        20,
        Some(1500),
        Some(2000),
        Some("2026-02-01"),
        Some("2026-02-05"),
    )
    .unwrap();
    assert_eq!(r.total, 1, "金额与日期同时命中才返回");
    assert_eq!(r.items[0].id, "tx-2");
}

#[test]
fn search_filters_exclude_soft_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1550);
    insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 1550);
    rebuild_search_index(&conn).unwrap();

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at='2026-02-03T00:00:00Z', version=version+1 WHERE id='tx-2'",
        [],
    )
    .unwrap();
    let r =
        search_transactions_internal(&conn, "", 1, 20, Some(1550), Some(1550), None, None).unwrap();
    assert_eq!(r.total, 1, "仅筛选查询同样排除软删除");
    assert_eq!(r.items[0].id, "tx-1");
}

#[test]
fn search_includes_hidden_account_and_all_kinds() {
    let conn = setup();
    // 黑洞账户（种子 无(CNY) 已存在）：income 入黑洞账户
    let hidden_id: String = conn
        .query_row(
            "SELECT id FROM accounts WHERE is_hidden=1 AND currency_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    insert_txn(
        &conn,
        "tx-hidden",
        &hidden_id,
        None,
        Some("退款入账"),
        "2026-02-01",
    );

    rebuild_search_index(&conn).unwrap();
    let r = search(&conn, "退款").unwrap();
    assert_eq!(r.total, 1, "黑洞账户交易可搜");
    assert_eq!(r.items[0].id, "tx-hidden");
}

#[test]
fn rebuild_is_idempotent_and_covers_legacy_data() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    // 存量数据：直接 SQL 插入（模拟 V005 迁移前），不调用应用层重建
    let n1 = rebuild_search_index(&conn).unwrap();
    assert_eq!(n1, 1);
    // 幂等：重复重建结果一致
    let n2 = rebuild_search_index(&conn).unwrap();
    assert_eq!(n2, 1);
    assert_eq!(search(&conn, "午餐").unwrap().total, 1);
}

#[test]
fn reconcile_rebuilds_when_counts_mismatch() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
    // FTS 为空（存量），counts 不匹配 → 全量重建
    reconcile_search_index(&conn).unwrap();
    assert_eq!(search(&conn, "午餐").unwrap().total, 1);
    // 再次对账：一致 → 走队列消费，结果不变
    reconcile_search_index(&conn).unwrap();
    assert_eq!(search(&conn, "午餐").unwrap().total, 1);
}

#[test]
fn account_rename_updates_searchable_content() {
    let conn = setup();
    insert_account(&conn, "acc-1", "招商银行", "bank", "CNY");
    insert_txn(&conn, "tx-1", "acc-1", None, None, "2026-02-01");
    rebuild_search_index(&conn).unwrap();
    assert_eq!(search(&conn, "招商").unwrap().total, 1);

    // 账户改名：触发器入队，消费后新名称生效
    conn.execute(
        "UPDATE accounts SET name='民生银行', updated_at='2026-02-02T00:00:00Z', version=version+1 WHERE id='acc-1'",
        [],
    )
    .unwrap();
    process_reindex_queue(&conn).unwrap();
    assert_eq!(search(&conn, "招商").unwrap().total, 0);
    assert_eq!(search(&conn, "民生").unwrap().total, 1);
    assert_eq!(search(&conn, "msyh").unwrap().total, 1);
}

#[test]
fn category_rename_does_not_affect_search() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    insert_category(&conn, "cat-1", "餐饮", "expense");
    insert_txn(&conn, "tx-1", "acc-1", Some("cat-1"), None, "2026-02-01");
    rebuild_search_index(&conn).unwrap();
    // 分类名不在索引中：分类名/拼音均不可搜
    assert_eq!(search(&conn, "餐饮").unwrap().total, 0);
    assert_eq!(search(&conn, "cy").unwrap().total, 0);

    // 分类改名不触发重建：索引内容与结果均不变
    conn.execute(
        "UPDATE categories SET name='美食', updated_at='2026-02-02T00:00:00Z', version=version+1 WHERE id='cat-1'",
        [],
    )
    .unwrap();
    process_reindex_queue(&conn).unwrap();
    assert_eq!(search(&conn, "美食").unwrap().total, 0);
}

#[test]
fn buy_transaction_indexed_with_account_name() {
    use crate::commands::transactions::insert_transaction;
    use crate::models::TransactionInput;
    let conn = setup();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('acc-inv','美股账户','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES ('inst-1','AAPL','stock','苹果','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();
    // buy 本位币折算走 Amount 接缝（issue #70）：补 1:1 汇率。
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-search','USD','CNY',1.0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();
    let input = TransactionInput {
        kind: "buy".into(),
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: "acc-inv".into(),
        to_account_id: None,
        category_id: None,
        refund_of_transaction_id: None,
        note: Some("加仓".into()),
        date: "2026-01-10".into(),
        instrument_id: Some("inst-1".into()),
        quantity: Some(10.0),
        price_cents: Some(10000),
        fee_cents: Some(0),
        idempotency_key: None,
    };
    let id = insert_transaction(&conn, input).unwrap();
    // 写入路径不做同步索引（ADR-0004 决策 #14）：消费队列后立即可搜
    process_reindex_queue(&conn).unwrap();
    assert_eq!(search(&conn, "加仓").unwrap().total, 1);
    assert_eq!(
        search(&conn, "美股账户").unwrap().total,
        1,
        "投资交易按账户名可搜（全部交易类型覆盖）"
    );
    let _ = id;
}
