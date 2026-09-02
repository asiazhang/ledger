//! 报表测试（issue #57 迁移后）：断言报表核心函数与 Amount 接缝的度量矩阵一致，
//! 不复制生产 SQL——期望值由 `signed_amount`（kind × measure）对夹具逐行求和得出。

use rusqlite::Connection;

use crate::db::{device_id, now_iso};
use crate::reports::{category_shares_rows, merchant_shares_rows, monthly_summary_rows};
use crate::transaction::amount::{Measure, TransactionKind, signed_amount};

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

/// 夹具一行 = 一笔交易（kind 用 Amount 接缝的 TransactionKind 枚举表述）。
struct TxRow {
    id: &'static str,
    kind: TransactionKind,
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
            kind: TransactionKind::Income,
            amount: 5000,
            category_id: category_id.clone(),
            date: "2026-01-05",
        },
        TxRow {
            id: "k-expense",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: category_id.clone(),
            date: "2026-01-06",
        },
        TxRow {
            id: "k-refund",
            kind: TransactionKind::Refund,
            amount: 300,
            category_id: category_id.clone(),
            date: "2026-01-07",
        },
        TxRow {
            id: "k-transfer",
            kind: TransactionKind::Transfer,
            amount: 800,
            category_id: category_id.clone(),
            date: "2026-01-08",
        },
        TxRow {
            id: "k-buy",
            kind: TransactionKind::Buy,
            amount: 2000,
            category_id: category_id.clone(),
            date: "2026-01-09",
        },
        TxRow {
            id: "k-sell",
            kind: TransactionKind::Sell,
            amount: 1500,
            category_id: category_id.clone(),
            date: "2026-01-10",
        },
        TxRow {
            id: "k-dividend",
            kind: TransactionKind::Dividend,
            amount: 60,
            category_id: category_id.clone(),
            date: "2026-01-11",
        },
        TxRow {
            id: "k-split",
            kind: TransactionKind::Split,
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
    let rows = monthly_summary_rows(&conn, 2026, None, None).unwrap();
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
    let rows = monthly_summary_rows(&conn, 2026, None, None).unwrap();
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
            kind: TransactionKind::Income,
            amount: 1000,
            category_id: None,
            date: "2025-12-31",
        },
        TxRow {
            id: "t2",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-01-20",
        },
        TxRow {
            id: "t3",
            kind: TransactionKind::Income,
            amount: 2000,
            category_id: None,
            date: "2026-02-10",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows_2025 = monthly_summary_rows(&conn, 2025, None, None).unwrap();
    assert_eq!(rows_2025.len(), 1);
    assert_eq!(rows_2025[0].income_cents, 1000);

    let rows_2026 = monthly_summary_rows(&conn, 2026, None, None).unwrap();
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
            kind: TransactionKind::Income,
            amount: 1000,
            category_id: None,
            date: "2026-01-15",
        },
    );
    conn.execute("UPDATE transactions SET is_deleted=1 WHERE id='t1'", [])
        .unwrap();
    assert!(
        monthly_summary_rows(&conn, 2026, None, None)
            .unwrap()
            .is_empty()
    );
}

// ---- 期间过滤（issue #411）：from/to 含边界，任一端存在即期间口径 ----

/// 年期间：按月分布（分组按月不变），年界外不计入。
/// 遗留 `year` 在期间口径下不参与，传 0 占位（下同）。
#[test]
fn monthly_summary_period_filters_by_range() {
    let conn = setup();
    insert_account(&conn, "acc");
    let fixture = vec![
        TxRow {
            id: "t-last-year",
            kind: TransactionKind::Income,
            amount: 800,
            category_id: None,
            date: "2025-12-31",
        },
        TxRow {
            id: "t-jan",
            kind: TransactionKind::Income,
            amount: 1000,
            category_id: None,
            date: "2026-01-20",
        },
        TxRow {
            id: "t-mar",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-03-05",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = monthly_summary_rows(&conn, 0, Some("2026-01-01"), Some("2026-12-31")).unwrap();
    assert_eq!(rows.len(), 2, "年期间内仅有流水的月份成行，年界外不计入");
    assert_eq!(rows[0].month, "2026-01");
    assert_eq!(rows[0].income_cents, 1000);
    assert_eq!(rows[1].month, "2026-03");
    assert_eq!(rows[1].expense_cents, 500);
}

/// 季期间：季度三个月界内成行、季外不计入（季 = 3 个月行）。
#[test]
fn monthly_summary_period_quarter_bounds() {
    let conn = setup();
    insert_account(&conn, "acc");
    let fixture = vec![
        TxRow {
            id: "t-q1-jan",
            kind: TransactionKind::Expense,
            amount: 300,
            category_id: None,
            date: "2026-01-10",
        },
        TxRow {
            id: "t-q1-end",
            kind: TransactionKind::Expense,
            amount: 400,
            category_id: None,
            date: "2026-03-31",
        },
        TxRow {
            id: "t-q2",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-04-01",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = monthly_summary_rows(&conn, 0, Some("2026-01-01"), Some("2026-03-31")).unwrap();
    assert_eq!(rows.len(), 2, "季界外（四季度首日）不计入");
    assert_eq!(rows[0].month, "2026-01");
    assert_eq!(rows[0].expense_cents, 300);
    assert_eq!(rows[1].month, "2026-03");
    assert_eq!(rows[1].expense_cents, 400);
}

/// 月期间：单行如实汇总（不切日粒度），起止边界日双双计入。
#[test]
fn monthly_summary_period_month_single_group_with_inclusive_bounds() {
    let conn = setup();
    insert_account(&conn, "acc");
    let fixture = vec![
        TxRow {
            id: "t-jan-last",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: None,
            date: "2026-01-31",
        },
        TxRow {
            id: "t-from",
            kind: TransactionKind::Expense,
            amount: 100,
            category_id: None,
            date: "2026-02-01",
        },
        TxRow {
            id: "t-mid",
            kind: TransactionKind::Expense,
            amount: 200,
            category_id: None,
            date: "2026-02-15",
        },
        TxRow {
            id: "t-to",
            kind: TransactionKind::Expense,
            amount: 400,
            category_id: None,
            date: "2026-02-28",
        },
        TxRow {
            id: "t-mar-first",
            kind: TransactionKind::Expense,
            amount: 1600,
            category_id: None,
            date: "2026-03-01",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = monthly_summary_rows(&conn, 0, Some("2026-02-01"), Some("2026-02-28")).unwrap();
    assert_eq!(rows.len(), 1, "月期间单行如实汇总（月期间不切日粒度）");
    assert_eq!(rows[0].month, "2026-02");
    assert_eq!(
        rows[0].expense_cents, 700,
        "起止边界日（02-01/02-28）双双计入"
    );
}

/// 期间口径优先：from/to 存在时遗留 year 不参与（他年流水不因 year 混入）。
#[test]
fn monthly_summary_period_overrides_legacy_year() {
    let conn = setup();
    insert_account(&conn, "acc");
    let fixture = vec![
        TxRow {
            id: "t-last-year",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: None,
            date: "2025-12-31",
        },
        TxRow {
            id: "t-jan",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-01-20",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = monthly_summary_rows(&conn, 2025, Some("2026-01-01"), Some("2026-12-31")).unwrap();
    assert_eq!(rows.len(), 1, "期间优先于遗留 year，2025 年流水不计入");
    assert_eq!(rows[0].month, "2026-01");
    assert_eq!(rows[0].expense_cents, 500);
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
    let rows = category_shares_rows(&conn, "expense", None, None, None, None).unwrap();
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
    let rows = category_shares_rows(&conn, "income", None, None, None, None).unwrap();
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
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-15",
        },
        TxRow {
            id: "t2",
            kind: TransactionKind::Expense,
            amount: 2000,
            category_id: Some(cat_id.clone()),
            date: "2026-02-10",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows_jan =
        category_shares_rows(&conn, "expense", Some("2026-01"), None, None, None).unwrap();
    assert_eq!(rows_jan.len(), 1);
    assert_eq!(rows_jan[0].amount_cents, 1000);
}

/// 年份过滤（issue #376）：传年份只统计所选年份的支出净值，他年不计入。
#[test]
fn category_shares_filters_by_year() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t-old",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: Some(cat_id.clone()),
            date: "2025-12-31",
        },
        TxRow {
            id: "t-new",
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-20",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(&conn, "expense", None, Some(2026), None, None).unwrap();
    assert_eq!(rows.len(), 1, "他年支出不计入所选年");
    assert_eq!(rows[0].amount_cents, 1000);
}

/// 退款冲减按所选年口径（issue #376）：退款以自身日期参与年份过滤，
/// 所选年内的退款冲减该年净额，他年退款不冲减所选年。
#[test]
fn category_shares_refund_offsets_within_selected_year_only() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t-expense-2026",
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-03-05",
        },
        TxRow {
            id: "t-refund-2026",
            kind: TransactionKind::Refund,
            amount: 300,
            category_id: Some(cat_id.clone()),
            date: "2026-03-08",
        },
        TxRow {
            id: "t-expense-2025",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: Some(cat_id.clone()),
            date: "2025-06-10",
        },
        TxRow {
            id: "t-refund-2025",
            kind: TransactionKind::Refund,
            amount: 150,
            category_id: Some(cat_id),
            date: "2025-08-01",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows_2026 = category_shares_rows(&conn, "expense", None, Some(2026), None, None).unwrap();
    assert_eq!(rows_2026.len(), 1);
    assert_eq!(
        rows_2026[0].amount_cents, 700,
        "2026 净额 = 1000 − 300，2025 退款不冲减"
    );
    let rows_2025 = category_shares_rows(&conn, "expense", None, Some(2025), None, None).unwrap();
    assert_eq!(rows_2025.len(), 1);
    assert_eq!(
        rows_2025[0].amount_cents, 350,
        "2025 净额 = 500 − 150，2026 退款不冲减"
    );
}

/// 缺省年份（None）保持全时段口径（issue #376：已发布 API 只增不改，既有调用方不回归）。
#[test]
fn category_shares_default_spans_all_years() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t-old",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: Some(cat_id.clone()),
            date: "2025-12-31",
        },
        TxRow {
            id: "t-new",
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-20",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(&conn, "expense", None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 1800, "缺省不传年份 = 全时段合计");
}

/// income 分支（`income_net` 口径）的年份过滤与 expense 同款 SQL 路径（issue #376）。
#[test]
fn category_shares_income_filters_by_year() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "income");
    let fixture = vec![
        TxRow {
            id: "t-old",
            kind: TransactionKind::Income,
            amount: 2000,
            category_id: Some(cat_id.clone()),
            date: "2025-05-01",
        },
        TxRow {
            id: "t-new",
            kind: TransactionKind::Income,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-02-20",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(&conn, "income", None, Some(2026), None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 1000);
}

/// month 与 year 过滤条件可叠加：占位符按条件追加顺序编号，参数一一对齐。
#[test]
fn category_shares_month_and_year_combine() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t-jan",
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-15",
        },
        TxRow {
            id: "t-feb",
            kind: TransactionKind::Expense,
            amount: 2000,
            category_id: Some(cat_id.clone()),
            date: "2026-02-10",
        },
        TxRow {
            id: "t-last-year-jan",
            kind: TransactionKind::Expense,
            amount: 4000,
            category_id: Some(cat_id),
            date: "2025-01-15",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows =
        category_shares_rows(&conn, "expense", Some("2026-01"), Some(2026), None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 1000, "同年同月才计入");
}

/// 期间过滤（issue #411）：界内净值成行，界外不计入；退款以自身日期参与期间过滤
///（界内退款冲减该期间净额，界外退款不冲减）。
#[test]
fn category_shares_filters_by_period_range() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t-in-range",
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-15",
        },
        TxRow {
            id: "t-refund-in-range",
            kind: TransactionKind::Refund,
            amount: 300,
            category_id: Some(cat_id.clone()),
            date: "2026-06-01",
        },
        TxRow {
            id: "t-out-of-range",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: Some(cat_id),
            date: "2025-12-31",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(
        &conn,
        "expense",
        None,
        None,
        Some("2026-01-01"),
        Some("2026-12-31"),
    )
    .unwrap();
    assert_eq!(rows.len(), 1, "界外（2025-12-31）支出不计入");
    assert_eq!(rows[0].amount_cents, 700, "1000 − 300，界内退款冲减");
}

/// 期间口径优先：from/to 存在时遗留 month/year 不参与。
#[test]
fn category_shares_period_overrides_legacy_month_year() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "expense");
    let fixture = vec![
        TxRow {
            id: "t-jan",
            kind: TransactionKind::Expense,
            amount: 1000,
            category_id: Some(cat_id.clone()),
            date: "2026-01-15",
        },
        TxRow {
            id: "t-feb",
            kind: TransactionKind::Expense,
            amount: 2000,
            category_id: Some(cat_id),
            date: "2026-02-10",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(
        &conn,
        "expense",
        Some("2026-01"),
        Some(2025),
        Some("2026-02-01"),
        Some("2026-02-28"),
    )
    .unwrap();
    assert_eq!(rows.len(), 1, "期间优先于遗留 month/year");
    assert_eq!(rows[0].amount_cents, 2000, "只计期间内（二月）支出");
}

/// income 分支（`income_net` 口径）与 expense 同款期间 SQL 路径（issue #411）。
#[test]
fn category_shares_income_filters_by_period_range() {
    let conn = setup();
    insert_account(&conn, "acc");
    let cat_id = first_category_id(&conn, "income");
    let fixture = vec![
        TxRow {
            id: "t-old",
            kind: TransactionKind::Income,
            amount: 2000,
            category_id: Some(cat_id.clone()),
            date: "2025-05-01",
        },
        TxRow {
            id: "t-new",
            kind: TransactionKind::Income,
            amount: 1000,
            category_id: Some(cat_id),
            date: "2026-02-20",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let rows = category_shares_rows(
        &conn,
        "income",
        None,
        None,
        Some("2026-01-01"),
        Some("2026-12-31"),
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 1000);
}

#[test]
fn category_shares_unclassified_shows_default_name() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_tx(
        &conn,
        &TxRow {
            id: "t1",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-01-15",
        },
    );
    let rows = category_shares_rows(&conn, "expense", None, None, None, None).unwrap();
    assert_eq!(rows[0].category_name, "未分类");
}

// ---- merchant_shares_rows：商户消费排行（净额口径，issue #192）----

fn insert_merchant(conn: &Connection, id: &str) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,1,?5,0)",
        rusqlite::params![id, format!("商户-{id}"), now, now, device_id()],
    )
    .unwrap();
}

/// 商户夹具一行 = 一笔带商户的交易（`amount` 即本位币分，与 `amount_native_cents` 同值）。
struct MerchantTxRow {
    id: &'static str,
    kind: TransactionKind,
    amount: i64,
    merchant_id: &'static str,
    date: &'static str,
}

fn insert_merchant_tx(conn: &Connection, r: &MerchantTxRow) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,merchant_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,'acc',NULL,NULL,?4,NULL,NULL,?5,?6,?7,1,?8,0)",
        rusqlite::params![r.id, r.kind.as_str(), r.amount, r.merchant_id, r.date, now, now, device_id()],
    )
    .unwrap();
}

/// 按月对商户夹具逐行求指定度量的和——期望值的唯一来源是度量矩阵，不是生产 SQL。
fn merchant_measure_sum(rows: &[MerchantTxRow], month: &str, measure: Measure) -> i64 {
    rows.iter()
        .filter(|r| r.date.starts_with(month))
        .map(|r| signed_amount(r.kind, r.amount, measure))
        .sum()
}

/// 退款冲减商户净额：`expense_net`（毛支出 − 退款）口径。
#[test]
fn merchant_shares_expense_net_subtracts_refund() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_merchant(&conn, "m-jd");
    let fixture = vec![
        MerchantTxRow {
            id: "t1",
            kind: TransactionKind::Expense,
            amount: 2000,
            merchant_id: "m-jd",
            date: "2026-03-05",
        },
        MerchantTxRow {
            id: "t2",
            kind: TransactionKind::Refund,
            amount: 300,
            merchant_id: "m-jd",
            date: "2026-03-08",
        },
    ];
    for r in &fixture {
        insert_merchant_tx(&conn, r);
    }
    let rows = merchant_shares_rows(&conn, 2026, None, None).unwrap();
    assert_eq!(rows.len(), 1, "退款归属同一商户，不新增行");
    let expected = merchant_measure_sum(&fixture, "2026-03", Measure::ExpenseNet);
    assert_eq!(
        rows[0].amount_cents, expected,
        "商户聚合应为 expense_net 口径"
    );
    assert_eq!(rows[0].amount_cents, 1700, "2000 − 300 退款冲减");
}

/// 覆盖全部 8 种 kind：只有 expense/refund 进商户排行；income（即使带商户）不计消费。
#[test]
fn merchant_shares_only_expense_kinds_contribute() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_merchant(&conn, "m-jd");
    let fixture = vec![
        MerchantTxRow {
            id: "t-income",
            kind: TransactionKind::Income,
            amount: 5000,
            merchant_id: "m-jd",
            date: "2026-03-01",
        },
        MerchantTxRow {
            id: "t-expense",
            kind: TransactionKind::Expense,
            amount: 1200,
            merchant_id: "m-jd",
            date: "2026-03-02",
        },
        MerchantTxRow {
            id: "t-refund",
            kind: TransactionKind::Refund,
            amount: 300,
            merchant_id: "m-jd",
            date: "2026-03-03",
        },
        MerchantTxRow {
            id: "t-transfer",
            kind: TransactionKind::Transfer,
            amount: 800,
            merchant_id: "m-jd",
            date: "2026-03-04",
        },
        MerchantTxRow {
            id: "t-buy",
            kind: TransactionKind::Buy,
            amount: 2000,
            merchant_id: "m-jd",
            date: "2026-03-05",
        },
        MerchantTxRow {
            id: "t-sell",
            kind: TransactionKind::Sell,
            amount: 1500,
            merchant_id: "m-jd",
            date: "2026-03-06",
        },
        MerchantTxRow {
            id: "t-dividend",
            kind: TransactionKind::Dividend,
            amount: 60,
            merchant_id: "m-jd",
            date: "2026-03-07",
        },
        MerchantTxRow {
            id: "t-split",
            kind: TransactionKind::Split,
            amount: 9999,
            merchant_id: "m-jd",
            date: "2026-03-08",
        },
    ];
    for r in &fixture {
        insert_merchant_tx(&conn, r);
    }
    let rows = merchant_shares_rows(&conn, 2026, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].amount_cents,
        merchant_measure_sum(&fixture, "2026-03", Measure::ExpenseNet),
        "只有 expense/refund 进消费口径"
    );
    assert_eq!(rows[0].amount_cents, 900);
}

/// 无商户关联的交易不进排行：净支出一律不虚增合计。
#[test]
fn merchant_shares_excludes_unmerchant_transactions() {
    let conn = setup();
    insert_account(&conn, "acc");
    // 无商户支出 + 带商户收入：均不产生排行行
    insert_tx(
        &conn,
        &TxRow {
            id: "t-no-merchant",
            kind: TransactionKind::Expense,
            amount: 500,
            category_id: None,
            date: "2026-03-05",
        },
    );
    insert_merchant(&conn, "m-jd");
    insert_merchant_tx(
        &conn,
        &MerchantTxRow {
            id: "t-income-with-merchant",
            kind: TransactionKind::Income,
            amount: 9000,
            merchant_id: "m-jd",
            date: "2026-03-06",
        },
    );
    let rows = merchant_shares_rows(&conn, 2026, None, None).unwrap();
    assert!(rows.is_empty(), "无商户支出与带商户收入都不进消费排行");
}

/// 多商户按净额降序排列，同额商户按名称次序稳定输出。
#[test]
fn merchant_shares_orders_desc() {
    let conn = setup();
    insert_account(&conn, "acc");
    for id in ["m-a", "m-b", "m-c"] {
        insert_merchant(&conn, id);
    }
    let fixture = vec![
        MerchantTxRow {
            id: "t1",
            kind: TransactionKind::Expense,
            amount: 100,
            merchant_id: "m-a",
            date: "2026-03-05",
        },
        MerchantTxRow {
            id: "t2",
            kind: TransactionKind::Expense,
            amount: 300,
            merchant_id: "m-b",
            date: "2026-03-06",
        },
        MerchantTxRow {
            id: "t3",
            kind: TransactionKind::Expense,
            amount: 200,
            merchant_id: "m-c",
            date: "2026-03-07",
        },
    ];
    for r in &fixture {
        insert_merchant_tx(&conn, r);
    }
    let rows = merchant_shares_rows(&conn, 2026, None, None).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.merchant_id.as_str()).collect();
    assert_eq!(names, vec!["m-b", "m-c", "m-a"], "应按净额降序");
}

/// 按年份过滤：只统计所选年份的商户净支出。
#[test]
fn merchant_shares_filters_by_year() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_merchant(&conn, "m-jd");
    let fixture = vec![
        MerchantTxRow {
            id: "t-old",
            kind: TransactionKind::Expense,
            amount: 1000,
            merchant_id: "m-jd",
            date: "2025-12-31",
        },
        MerchantTxRow {
            id: "t-new",
            kind: TransactionKind::Expense,
            amount: 700,
            merchant_id: "m-jd",
            date: "2026-01-02",
        },
    ];
    for r in &fixture {
        insert_merchant_tx(&conn, r);
    }
    let rows = merchant_shares_rows(&conn, 2026, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 700);
}

/// 期间过滤（issue #411）：界内净支出进排行，界外不计入。
#[test]
fn merchant_shares_filters_by_period() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_merchant(&conn, "m-jd");
    insert_merchant(&conn, "m-tt");
    let fixture = vec![
        MerchantTxRow {
            id: "t-old",
            kind: TransactionKind::Expense,
            amount: 800,
            merchant_id: "m-jd",
            date: "2025-12-31",
        },
        MerchantTxRow {
            id: "t-jan",
            kind: TransactionKind::Expense,
            amount: 1000,
            merchant_id: "m-jd",
            date: "2026-01-05",
        },
        MerchantTxRow {
            id: "t-feb",
            kind: TransactionKind::Expense,
            amount: 700,
            merchant_id: "m-tt",
            date: "2026-02-10",
        },
    ];
    for r in &fixture {
        insert_merchant_tx(&conn, r);
    }
    let rows = merchant_shares_rows(&conn, 0, Some("2026-01-01"), Some("2026-12-31")).unwrap();
    assert_eq!(rows.len(), 2, "界外（2025-12-31）支出不计入");
    assert_eq!(rows[0].merchant_id, "m-jd");
    assert_eq!(rows[0].amount_cents, 1000);
    assert_eq!(rows[1].merchant_id, "m-tt");
    assert_eq!(rows[1].amount_cents, 700);
}

/// 软删商户的历史引用照常统计显示（ADR-0028：历史引用保留，改名/软删不回刷历史行）。
#[test]
fn merchant_shares_includes_soft_deleted_merchant_history() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_merchant(&conn, "m-jd");
    insert_merchant_tx(
        &conn,
        &MerchantTxRow {
            id: "t1",
            kind: TransactionKind::Expense,
            amount: 1000,
            merchant_id: "m-jd",
            date: "2026-03-05",
        },
    );
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='m-jd'", [])
        .unwrap();
    let rows = merchant_shares_rows(&conn, 2026, None, None).unwrap();
    assert_eq!(rows.len(), 1, "软删商户的历史消费照常进排行");
    assert_eq!(rows[0].amount_cents, 1000);
}

// ---- query_report_date_range：日期极值范围（issue #266 / #389）----

use crate::reports::query_report_date_range;

#[test]
fn date_range_spans_earliest_to_latest_date() {
    let conn = setup();
    insert_account(&conn, "acc");
    for (id, date) in [
        ("t-old", "2024-03-01"),
        ("t-mid", "2025-08-15"),
        ("t-new", "2026-01-20"),
    ] {
        insert_tx(
            &conn,
            &TxRow {
                id,
                kind: TransactionKind::Expense,
                amount: 100,
                category_id: None,
                date,
            },
        );
    }
    let range = query_report_date_range(&conn).unwrap();
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (Some("2024-03-01"), Some("2026-01-20")),
        "起点为最早流水日期，终点为最新流水日期"
    );
}

#[test]
fn date_range_end_expanded_by_future_data() {
    let conn = setup();
    insert_account(&conn, "acc");
    for (id, date) in [("t-past", "2025-05-01"), ("t-future", "2027-11-30")] {
        insert_tx(
            &conn,
            &TxRow {
                id,
                kind: TransactionKind::Income,
                amount: 100,
                category_id: None,
                date,
            },
        );
    }
    let range = query_report_date_range(&conn).unwrap();
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (Some("2025-05-01"), Some("2027-11-30")),
        "未来日期流水如实撑大终点"
    );
}

#[test]
fn date_range_empty_db_returns_none_pair() {
    let conn = setup();
    insert_account(&conn, "acc");
    let range = query_report_date_range(&conn).unwrap();
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (None, None),
        "空库回退双 None (null)"
    );
}

#[test]
fn date_range_excludes_deleted() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_tx(
        &conn,
        &TxRow {
            id: "t-deleted",
            kind: TransactionKind::Expense,
            amount: 100,
            category_id: None,
            date: "2020-01-01",
        },
    );
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id='t-deleted'",
        [],
    )
    .unwrap();
    let range = query_report_date_range(&conn).unwrap();
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (None, None),
        "软删交易不参与范围"
    );
}

#[test]
fn date_range_counts_all_kinds_by_date() {
    let conn = setup();
    insert_account(&conn, "acc");
    insert_tx(
        &conn,
        &TxRow {
            id: "t-transfer",
            kind: TransactionKind::Transfer,
            amount: 800,
            category_id: None,
            date: "2023-07-01",
        },
    );
    let range = query_report_date_range(&conn).unwrap();
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (Some("2023-07-01"), Some("2023-07-01")),
        "任何 kind 的未删交易日期都参与范围（转账也一样）"
    );
}
