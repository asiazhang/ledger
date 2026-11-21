use std::collections::HashSet;

use ledger_infra::db::query::query_one;

use crate::amount::{FxRateSource, TransactionKind};
use crate::model::{ROW_COLUMNS, Transaction};
use crate::tests::common::seed_category;
use tauri_app_lib::test_support;

const EXPECTED_COLUMN_COUNT: usize = 22;

const EXCLUDED_FROM_READ_COLUMNS: &[&str] = &["dedup_hash", "idempotency_key"];

fn insert_probe_rows(conn: &rusqlite::Connection, category_id: &str) {
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

    assert_eq!(ROW_COLUMNS.len(), EXPECTED_COLUMN_COUNT);

    let sql = format!(
        "SELECT {} FROM transactions WHERE id = 'rowmap-target'",
        ROW_COLUMNS.join(",")
    );
    let tx = query_one::<Transaction, _>(&conn, &sql, rusqlite::params![])
        .unwrap()
        .expect("探针行应读回");

    assert_eq!(tx.id, "rowmap-target");
    assert_eq!(tx.kind, TransactionKind::Expense);
    assert_eq!(tx.amount_cents, 1101);
    assert_eq!(tx.currency_code, "CNY");
    assert_eq!(tx.amount_native_cents, 2202);
    assert_eq!(tx.account_id.as_deref(), Some("acc-rowmap"));
    assert_eq!(tx.to_account_id.as_deref(), Some("acc-rowmap-to"));
    assert_eq!(tx.funding_account_id.as_deref(), Some("acc-rowmap-fund"));
    assert_eq!(tx.category_id.as_deref(), Some(category_id.as_str()));
    assert_eq!(tx.refund_of_transaction_id, Some("rowmap-origin".into()));
    assert_eq!(tx.note.as_deref(), Some("rowmap-note"));
    assert_eq!(tx.date, "2020-02-02");
    assert_eq!(tx.created_at, "2020-02-02T00:00:00Z");
    assert_eq!(tx.updated_at, "2020-02-02T00:00:01Z");
    assert_eq!(tx.version, 3303);
    assert_eq!(tx.device_id, "rowmap-device");
    assert!(!tx.is_deleted);
    assert_eq!(tx.merchant_id.as_deref(), Some("rowmap-merchant"));
    assert_eq!(tx.policy_id, None);
    assert_eq!(tx.fx_rate_used, Some(4.404));
    assert_eq!(tx.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(tx.source_order_no.as_deref(), Some("rowmap-order"));
}

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
