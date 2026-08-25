//! 报表测试（issue #57 迁移后）：断言报表核心函数与 Amount 接缝的度量矩阵一致，
//! 不复制生产 SQL——期望值由 `signed_amount`（kind × measure）对夹具逐行求和得出。

use rusqlite::Connection;

use crate::commands::reports::{category_shares_rows, monthly_summary_rows};
use crate::db::{device_id, now_iso};
use crate::transaction::amount::{Kind, Measure, signed_amount};

fn setup() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn insert_account(conn: &Connection, id: &str) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'cash','CNY',0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "测试账户", now, now, 1, device_id()],
    ).unwrap();
}

/// 夹具一行 = 一笔交易（kind 用 Amount 接缝的 Kind 枚举表述）。
struct TxRow {
    id: &'static str,
    kind: Kind,
    amount: i64,
    category_id: Option<String>,
    date: &'static str,
}

fn insert_tx(conn: &Connection, r: &TxRow) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,'acc',NULL,?4,NULL,NULL,?5,?6,?7,1,?8,0)",
        rusqlite::params![r.id, r.kind.as_str(), r.amount, r.category_id, r.date, now, now, device_id()],
    )
    .unwrap();
}

/// 按月对夹具逐行求指定度量的和——期望值的唯一来源是度量矩阵，不是生产 SQL。
fn measure_sum(rows: &[TxRow], month: &str, measure: Measure) -> i64 {
    rows.iter()
        .filter(|r| r.date.starts_with(month))
        .map(|r| signed_amount(r.kind, r.amount, measure))
        .sum()
}

/// 覆盖全部 8 种 kind 的夹具：验证月度三列恒等于度量矩阵口径。
fn all_kinds_fixture(category_id: Option<&str>) -> Vec<TxRow> {
    let category_id = category_id.map(|s| s.to_string());
    vec![
        TxRow {
            id: "k-income",
            kind: Kind::Income,
            amount: 5000,
            category_id: category_id.clone(),
            date: "2026-01-05",
        },
        TxRow {
            id: "k-expense",
            kind: Kind::Expense,
            amount: 1200,
            category_id: category_id.clone(),
            date: "2026-01-06",
        },
        TxRow {
            id: "k-refund",
            kind: Kind::Refund,
            amount: 300,
            category_id: category_id.clone(),
            date: "2026-01-07",
        },
        TxRow {
            id: "k-transfer",
            kind: Kind::Transfer,
            amount: 800,
            category_id: category_id.clone(),
            date: "2026-01-08",
        },
        TxRow {
            id: "k-buy",
            kind: Kind::Buy,
            amount: 2000,
            category_id: category_id.clone(),
            date: "2026-01-09",
        },
        TxRow {
            id: "k-sell",
            kind: Kind::Sell,
            amount: 1500,
            category_id: category_id.clone(),
            date: "2026-01-10",
        },
        TxRow {
            id: "k-dividend",
            kind: Kind::Dividend,
            amount: 60,
            category_id: category_id.clone(),
            date: "2026-01-11",
        },
        TxRow {
            id: "k-split",
            kind: Kind::Split,
            amount: 9999,
            category_id,
            date: "2026-01-12",
        },
    ]
}

// ---- monthly_summary_rows：毛值三列，单一口径 ----

#[test]
fn monthly_summary_empty_when_no_transactions() {
    let conn = setup();
    insert_account(&conn, "acc");
    let rows = monthly_summary_rows(&conn, 2026).unwrap();
    assert!(rows.is_empty());
}

/// 月度三列对全部 kind 一致：
/// - income 列 = `income_net`（收入 + 分红）
/// - expense 列 = 毛支出 = `expense_net + refund_gross`（净值恒等式）
/// - refund 列 = `refund_gross`
#[test]
fn monthly_summary_columns_match_measures_for_all_kinds() {
    let conn = setup();
    insert_account(&conn, "acc");
    let fixture = all_kinds_fixture(None);
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = monthly_summary_rows(&conn, 2026).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].month, "2026-01");
    assert_eq!(
        rows[0].income_cents,
        measure_sum(&fixture, "2026-01", Measure::IncomeNet),
        "income 列应为 income_net 口径（含分红）"
    );
    assert_eq!(rows[0].income_cents, 5060, "收入 + 分红 = 5000 + 60");
    assert_eq!(
        rows[0].expense_cents,
        measure_sum(&fixture, "2026-01", Measure::ExpenseNet)
            + measure_sum(&fixture, "2026-01", Measure::RefundGross),
        "expense 列应为毛支出 = expense_net + refund_gross"
    );
    assert_eq!(rows[0].expense_cents, 1200, "毛支出不含退款冲减");
    assert_eq!(
        rows[0].refund_cents,
        measure_sum(&fixture, "2026-01", Measure::RefundGross),
        "refund 列应为 refund_gross 口径"
    );
    assert_eq!(rows[0].refund_cents, 300);
}

#[test]
fn monthly_summary_groups_by_month_and_filters_by_year() {
    let conn = setup();
    insert_account(&conn, "acc");
    let fixture = vec![
        TxRow {
            id: "t1",
            kind: Kind::Income,
            amount: 1000,
            category_id: None,
            date: "2025-12-31",
        },
        TxRow {
            id: "t2",
            kind: Kind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-01-20",
        },
        TxRow {
            id: "t3",
            kind: Kind::Income,
            amount: 2000,
            category_id: None,
            date: "2026-02-10",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows_2025 = monthly_summary_rows(&conn, 2025).unwrap();
    assert_eq!(rows_2025.len(), 1);
    assert_eq!(rows_2025[0].income_cents, 1000);

    let rows_2026 = monthly_summary_rows(&conn, 2026).unwrap();
    assert_eq!(rows_2026.len(), 2);
    assert_eq!(rows_2026[0].month, "2026-01");
    assert_eq!(rows_2026[0].expense_cents, 500);
    assert_eq!(rows_2026[1].month, "2026-02");
    assert_eq!(rows_2026[1].income_cents, 2000);
}

#[test]
fn monthly_summary_excludes_deleted() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_tx(
        &conn,
        &TxRow {
            id: "t1",
            kind: Kind::Income,
            amount: 1000,
            category_id: None,
            date: "2026-01-15",
        },
    );
    conn.execute("UPDATE transactions SET is_deleted=1 WHERE id='t1'", [])
        .unwrap();
    assert!(monthly_summary_rows(&conn, 2026).unwrap().is_empty());
}

// ---- category_shares_rows：净值口径 ----

fn first_category_id(conn: &Connection, kind: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE kind=?1 AND parent_id IS NULL ORDER BY created_at LIMIT 1",
        [kind],
        |r| r.get(0),
    )
    .unwrap()
}

/// 支出分类净值 = `expense_net`（毛支出 − 退款）；投资类（buy/sell）不进经营收支。
#[test]
fn category_shares_expense_net_subtracts_refund() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = all_kinds_fixture(Some(&cat_id));
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(&conn, "expense", None).unwrap();
    assert_eq!(rows.len(), 1, "只有 expense/refund 计入净值口径");
    let expected = measure_sum(&fixture, "2026-01", Measure::ExpenseNet);
    assert_eq!(
        rows[0].amount_cents, expected,
        "分类聚合应为 expense_net 口径"
    );
    assert_eq!(rows[0].amount_cents, 900, "1200 − 300 退款");
}

/// 收入分类净值 = `income_net`（收入 + 分红）；分红计入收入报表。
#[test]
fn category_shares_income_net_includes_dividend() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "income");
    let fixture = all_kinds_fixture(Some(&cat_id));
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(&conn, "income", None).unwrap();
    assert_eq!(rows.len(), 1, "只有 income/dividend 计入净值口径");
    let expected = measure_sum(&fixture, "2026-01", Measure::IncomeNet);
    assert_eq!(
        rows[0].amount_cents, expected,
        "分类聚合应为 income_net 口径"
    );
    assert_eq!(rows[0].amount_cents, 5060, "收入 + 分红");
}

#[test]
fn category_shares_filters_by_month() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t1",
            kind: Kind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-15",
        },
        TxRow {
            id: "t2",
            kind: Kind::Expense,
            amount: 2000,
            category_id: Some(cat_id.clone()),
            date: "2026-02-10",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows_jan = category_shares_rows(&conn, "expense", Some("2026-01")).unwrap();
    assert_eq!(rows_jan.len(), 1);
    assert_eq!(rows_jan[0].amount_cents, 1000);
}

#[test]
fn category_shares_unclassified_shows_default_name() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_tx(
        &conn,
        &TxRow {
            id: "t1",
            kind: Kind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-01-15",
        },
    );
    let rows = category_shares_rows(&conn, "expense", None).unwrap();
    assert_eq!(rows[0].category_name, "未分类");
}
