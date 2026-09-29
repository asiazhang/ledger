//! 全列 SELECT 清单 ↔ FromRow 位置下标映射钉住（issue #1880）。
//!
//! [`ROW_COLUMNS`]（模型共享语义区）与 `Transaction::from_row` 的位置下标一一对应，
//! 错位是静默数据错读——本测试用「每列可辨识探针值」的全列行做确定性钉住：任何两列
//! 顺序调整未同步 FromRow，两列探针值即错落变红；新增表列完全漏改（既不进常量也
//! 不进读模型）由 `row_columns_cover_table_columns` 的 schema 对齐断言变红。

use std::collections::HashSet;

use ledger_infra::db::query::query_one;

use crate::amount::{FxRateSource, TransactionKind};
use crate::model::{ROW_COLUMNS, Transaction};
use crate::tests::common::seed_category;
use tauri_app_lib::test_support;

/// 新增 `transactions` 列的强制经过点：新增列须同步更新 [`ROW_COLUMNS`]、
/// `Transaction::from_row` 下标、写入侧列清单与本测试的探针行 + 字段断言；
/// 本断言不一致即红，防止「只加常量」静默通过。
const EXPECTED_COLUMN_COUNT: usize = 22;

/// 有意不进全列读模型的表列闭集（schema 对齐断言的显式例外）：幂等身份列由命令层
/// 落库后回写（writer 只管行本体，见 `write/writer.rs` 文件头），读回无消费面。
/// 新增表列若属同类「有意不读回」，须显式登记于此并说明口径。
const EXCLUDED_FROM_READ_COLUMNS: &[&str] = &["dedup_hash", "idempotency_key"];

/// 探针行：值型列各持可辨识值，仅 `policy_id` 留 NULL（保单种子需 insurer 链，
/// 不成比例）——唯一 NULL 列两侧（merchant / fx_rate_used）均有值，任何两列互换
/// 或单列插入都会撞上错列探针变红。直接落库绕过写入协议：本测试钉的是读回契约，
/// 不是写入行为。
fn insert_probe_rows(conn: &rusqlite::Connection, category_id: &str) {
    // refund_of_transaction_id 的引用目标（自身外键）：最小可行原支出行。
    conn.execute(
        "INSERT INTO transactions \
         (id, kind, amount_cents, currency_code, amount_native_cents, account_id, \
          date, created_at, updated_at, device_id) \
         VALUES ('rowmap-origin', 'expense', 9901, 'CNY', 9902, 'acc-rowmap', \
                 '2020-02-01', '2020-02-01T00:00:00Z', '2020-02-01T00:00:00Z', \
                 'rowmap-device')",
        [],
    )
    .unwrap();
    // 探针外键目标先落（target 行引用其 id）：两枚出资侧账户 + 商户（轻量裸插，
    // 仅本测试消费）。
    test_support::seed_account(conn, "acc-rowmap-to", "转入", "cash", "CNY", 0);
    test_support::seed_account(conn, "acc-rowmap-fund", "出资", "cash", "CNY", 0);
    conn.execute(
        "INSERT INTO merchants (id, name, created_at, updated_at, device_id) \
         VALUES ('rowmap-merchant', '行映射商户', '2020-02-02T00:00:00Z', \
                 '2020-02-02T00:00:00Z', 'rowmap-device')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transactions \
         (id, kind, amount_cents, currency_code, amount_native_cents, account_id, \
          to_account_id, funding_account_id, category_id, refund_of_transaction_id, \
          note, date, created_at, updated_at, version, device_id, is_deleted, \
          merchant_id, policy_id, fx_rate_used, fx_rate_source, source_order_no) \
         VALUES ('rowmap-target', 'expense', 1101, 'CNY', 2202, 'acc-rowmap', \
                 'acc-rowmap-to', 'acc-rowmap-fund', ?1, 'rowmap-origin', \
                 'rowmap-note', '2020-02-02', '2020-02-02T00:00:00Z', \
                 '2020-02-02T00:00:01Z', 3303, 'rowmap-device', 0, \
                 'rowmap-merchant', NULL, 4.404, 'series', 'rowmap-order')",
        rusqlite::params![category_id],
    )
    .unwrap();
}

#[test]
fn row_columns_match_from_row_positions() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-rowmap", "现金", "cash", "CNY", 0);
    let category_id = seed_category(&conn, "行映射分类", "expense");
    insert_probe_rows(&conn, &category_id);

    // 列数钉住（新增列必须更新本测试，见常量注记）。
    assert_eq!(ROW_COLUMNS.len(), EXPECTED_COLUMN_COUNT);

    // 用常量拼 SELECT 再经 FromRow 读回：列序错位时探针值落在错误字段上。
    let sql = format!(
        "SELECT {} FROM transactions WHERE id = 'rowmap-target'",
        ROW_COLUMNS.join(",")
    );
    let tx = query_one::<Transaction, _>(&conn, &sql, rusqlite::params![])
        .unwrap()
        .expect("探针行应读回");

    // 列名 → 字段值断言（与 ROW_COLUMNS 顺序一一对应）。
    assert_eq!(tx.id, "rowmap-target"); // id
    assert_eq!(tx.kind, TransactionKind::Expense); // kind
    assert_eq!(tx.amount_cents, 1101); // amount_cents
    assert_eq!(tx.currency_code, "CNY"); // currency_code
    assert_eq!(tx.amount_native_cents, 2202); // amount_native_cents
    assert_eq!(tx.account_id.as_deref(), Some("acc-rowmap")); // account_id
    assert_eq!(tx.to_account_id.as_deref(), Some("acc-rowmap-to")); // to_account_id
    assert_eq!(tx.funding_account_id.as_deref(), Some("acc-rowmap-fund")); // funding_account_id
    assert_eq!(tx.category_id.as_deref(), Some(category_id.as_str())); // category_id
    assert_eq!(tx.refund_of_transaction_id, Some("rowmap-origin".into())); // refund_of_transaction_id
    assert_eq!(tx.note.as_deref(), Some("rowmap-note")); // note
    assert_eq!(tx.date, "2020-02-02"); // date
    assert_eq!(tx.created_at, "2020-02-02T00:00:00Z"); // created_at
    assert_eq!(tx.updated_at, "2020-02-02T00:00:01Z"); // updated_at
    assert_eq!(tx.version, 3303); // version
    assert_eq!(tx.device_id, "rowmap-device"); // device_id
    assert!(!tx.is_deleted); // is_deleted
    assert_eq!(tx.merchant_id.as_deref(), Some("rowmap-merchant")); // merchant_id
    assert_eq!(tx.policy_id, None); // policy_id（唯一 NULL 探针，见 insert_probe_rows 注记）
    assert_eq!(tx.fx_rate_used, Some(4.404)); // fx_rate_used
    assert_eq!(tx.fx_rate_source, Some(FxRateSource::Series)); // fx_rate_source
    assert_eq!(tx.source_order_no.as_deref(), Some("rowmap-order")); // source_order_no
}

/// schema 对齐（Spec 审查 #1880 补强）：`ROW_COLUMNS` ∪ 显式例外 ≡ `transactions`
/// 表列全集。迁移新增表列而常量与 FromRow 均未同步时（新列永不读回、无错位产生），
/// 集合差非空即红——闭合「完全漏改」这一最静默形态。
#[test]
fn row_columns_cover_table_columns() {
    let conn = test_support::open();
    let mut stmt = conn.prepare("PRAGMA table_info(transactions)").unwrap();
    let table_columns: HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    let mut expected: HashSet<String> = ROW_COLUMNS.iter().map(|c| c.to_string()).collect();
    expected.extend(EXCLUDED_FROM_READ_COLUMNS.iter().map(|c| c.to_string()));
    assert_eq!(
        table_columns,
        expected,
        "表列与读回清单出现集合差：新表列未进 ROW_COLUMNS（或例外未登记 EXCLUDED_FROM_READ_COLUMNS），\n\
         表列差 = {:?}，清单差 = {:?}",
        table_columns.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&table_columns).collect::<Vec<_>>(),
    );
}
