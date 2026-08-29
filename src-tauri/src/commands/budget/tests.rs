//! 预算测试（issue #58 迁移后；issue #182 滚动窗口）：断言预算核心函数与
//! Amount 接缝的度量矩阵一致，不复制生产 SQL——期望值由 `signed_amount`
//! （kind × measure）对夹具逐行求和得出。窗口口径经注入 `today` 驱动。

use chrono::NaiveDate;
use rusqlite::Connection;

use crate::commands::budget::{budget_progress_rows, create_budget_internal};
use crate::db::{device_id, now_iso};
use crate::error::AppError;
use crate::models::{BudgetInput, BudgetPeriod};
use crate::transaction::amount::{Measure, TransactionKind, signed_amount};

fn setup() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn first_expense_category_id(conn: &Connection) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE kind='expense' AND parent_id IS NULL ORDER BY created_at LIMIT 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn first_expense_subcategory_id(conn: &Connection, parent_id: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE parent_id=?1 ORDER BY created_at LIMIT 1",
        rusqlite::params![parent_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn insert_budget(
    conn: &Connection,
    id: &str,
    category_id: &str,
    period: &str,
    amount_cents: i64,
    start_date: &str,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![id, category_id, period, amount_cents, start_date, now, now, 1, device_id()],
    ).unwrap();
}

/// 注入的「今天」：所有窗口口径测试共用，夹具日期围绕它铺开。
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

fn insert_dummy_account(conn: &Connection) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('dummy','虚拟账户','cash','CNY',0,?1,?2,?3,?4,0)",
        rusqlite::params![now, now, 1, device_id()],
    ).unwrap();
}

/// 夹具一行 = 一笔交易（kind 用 Amount 接缝的 TransactionKind 枚举表述）。
struct TxRow {
    id: &'static str,
    kind: TransactionKind,
    amount: i64,
    category_id: String,
    date: &'static str,
}

fn insert_tx(conn: &Connection, r: &TxRow) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,'dummy',NULL,?4,NULL,NULL,?5,?6,?7,1,?8,0)",
        rusqlite::params![r.id, r.kind.as_str(), r.amount, r.category_id, r.date, now, now, device_id()],
    )
    .unwrap();
}

/// 按年/月前缀对夹具逐行求 `expense_net` 度量和——期望值的唯一来源是度量矩阵，不是生产 SQL。
/// 月预算窗口传 "YYYY-MM"，年预算窗口传 "YYYY"。
fn expense_net_sum(rows: &[TxRow], prefix: &str) -> i64 {
    rows.iter()
        .filter(|r| r.date.starts_with(prefix))
        .map(|r| signed_amount(r.kind, r.amount, Measure::ExpenseNet))
        .sum()
}

/// 覆盖全部 8 种 kind 的夹具：验证预算 spent 恒等于 expense_net 口径。
fn all_kinds_fixture(category_id: &str) -> Vec<TxRow> {
    let category_id = category_id.to_string();
    vec![
        TxRow {
            id: "k-income",
            kind: TransactionKind::Income,
            amount: 5000,
            category_id: category_id.clone(),
            date: "2026-07-05",
        },
        TxRow {
            id: "k-expense",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: category_id.clone(),
            date: "2026-07-06",
        },
        TxRow {
            id: "k-refund",
            kind: TransactionKind::Refund,
            amount: 300,
            category_id: category_id.clone(),
            date: "2026-07-07",
        },
        TxRow {
            id: "k-transfer",
            kind: TransactionKind::Transfer,
            amount: 800,
            category_id: category_id.clone(),
            date: "2026-07-08",
        },
        TxRow {
            id: "k-buy",
            kind: TransactionKind::Buy,
            amount: 2000,
            category_id: category_id.clone(),
            date: "2026-07-09",
        },
        TxRow {
            id: "k-sell",
            kind: TransactionKind::Sell,
            amount: 1500,
            category_id: category_id.clone(),
            date: "2026-07-10",
        },
        TxRow {
            id: "k-dividend",
            kind: TransactionKind::Dividend,
            amount: 60,
            category_id: category_id.clone(),
            date: "2026-07-11",
        },
        TxRow {
            id: "k-split",
            kind: TransactionKind::Split,
            amount: 9999,
            category_id,
            date: "2026-07-12",
        },
    ]
}

// ---- 预算 CRUD（薄壳 SQL 层） ----

#[test]
fn list_budgets_empty_initially() {
    let conn = setup();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn create_budget_and_list() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-1", &cat_id, "monthly", 50000, "2026-07-01");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn delete_budget_soft_deletes() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-2", &cat_id, "monthly", 50000, "2026-07-01");
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r
            .get(0))
            .unwrap(),
        1,
    );
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["budget-2", now_iso(), device_id()],
    )
    .unwrap();
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r
            .get(0))
            .unwrap(),
        0,
    );
}

// ---- create_budget_internal：写入校验与同分类同周期查重（issue #183） ----

fn budget_input(category_id: &str, period: Option<BudgetPeriod>, amount_cents: i64) -> BudgetInput {
    BudgetInput {
        category_id: category_id.into(),
        period,
        amount_cents,
        start_date: "2026-07-01".into(),
    }
}

fn first_income_category_id(conn: &Connection) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE kind='income' AND parent_id IS NULL ORDER BY created_at LIMIT 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn budget_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM budgets", [], |r| r.get(0))
        .unwrap()
}

/// 底线一：预算金额必须为正数，0 与负数均拒绝，不落库。
#[test]
fn create_budget_rejects_non_positive_amount() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    for amount in [0, -100] {
        let err = create_budget_internal(&conn, &budget_input(&cat_id, None, amount)).unwrap_err();
        assert!(
            matches!(err, AppError::Invalid(ref m) if m.contains("预算金额必须为正数")),
            "金额 {amount} 应被拒绝: {err:?}"
        );
    }
    assert_eq!(budget_count(&conn), 0);
}

/// 底线二：收入分类不可设预算。
#[test]
fn create_budget_rejects_income_category() {
    let conn = setup();
    let cat_id = first_income_category_id(&conn);
    let err = create_budget_internal(&conn, &budget_input(&cat_id, None, 1000)).unwrap_err();
    assert!(
        matches!(err, AppError::Invalid(ref m) if m.contains("预算只能设置在支出分类上")),
        "{err:?}"
    );
    assert_eq!(budget_count(&conn), 0);
}

/// 不存在的分类同样拒绝（NotFound）。
#[test]
fn create_budget_rejects_missing_category() {
    let conn = setup();
    let err = create_budget_internal(&conn, &budget_input("no-such-cat", None, 1000)).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
}

/// 底线三：同分类同周期重复创建明确拒绝，且原预算数据不受影响。
#[test]
fn create_budget_rejects_duplicate_monthly_and_keeps_original() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-dup", &cat_id, "monthly", 50000, "2026-07-01");
    let err = create_budget_internal(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Monthly), 1000),
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Invalid(ref m) if m == "该分类已存在按月预算"),
        "{err:?}"
    );
    let (amount, start): (i64, String) = conn
        .query_row(
            "SELECT amount_cents,start_date FROM budgets WHERE id='budget-dup'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount, 50000, "原预算金额不得被覆盖");
    assert_eq!(start, "2026-07-01", "原预算开始日期不得被改");
    assert_eq!(budget_count(&conn), 1, "不得插入第二行");
}

/// 同分类同周期查重对年预算同样生效，提示「按年」。
#[test]
fn create_budget_rejects_duplicate_yearly() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-dup-y",
        &cat_id,
        "yearly",
        100000,
        "2026-01-01",
    );
    let err = create_budget_internal(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Yearly), 1000),
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Invalid(ref m) if m == "该分类已存在按年预算"),
        "{err:?}"
    );
}

/// 同分类不同周期共存（月预算 + 年预算互不冲突）。
#[test]
fn create_budget_allows_same_category_different_period() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-m", &cat_id, "monthly", 50000, "2026-07-01");
    let id = create_budget_internal(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Yearly), 100000),
    )
    .unwrap();
    assert_eq!(budget_count(&conn), 2);
    let period: String = conn
        .query_row("SELECT period FROM budgets WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(period, "yearly");
}

/// 周期缺省为月度。
#[test]
fn create_budget_defaults_to_monthly() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    let id = create_budget_internal(&conn, &budget_input(&cat_id, None, 1000)).unwrap();
    let period: String = conn
        .query_row("SELECT period FROM budgets WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(period, "monthly");
}

/// 软删后同分类同周期可重新创建（查重只看未删除行）。
#[test]
fn create_budget_allows_recreate_after_soft_delete() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-old", &cat_id, "monthly", 50000, "2026-07-01");
    conn.execute("UPDATE budgets SET is_deleted=1 WHERE id='budget-old'", [])
        .unwrap();
    create_budget_internal(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Monthly), 1000),
    )
    .unwrap();
    assert_eq!(budget_count(&conn), 2);
}

// ---- budget_progress_rows：expense_net 口径 ----

#[test]
fn budget_progress_zero_when_no_transactions() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-3", &cat_id, "monthly", 50000, "2026-07-01");
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].budget.amount_cents, 50000);
    assert_eq!(results[0].spent_cents, 0);
    assert!(!results[0].over_budget);
}

/// 预算 spent = `expense_net`（毛支出 − 退款）对全部 kind 一致；
/// 投资/转账/收入/拆股类不进预算口径。
#[test]
fn budget_progress_spent_matches_expense_net_for_all_kinds() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-4", &cat_id, "monthly", 50000, "2026-07-01");
    let fixture = all_kinds_fixture(&cat_id);
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026-07"),
        "spent 应为 expense_net 口径"
    );
    assert_eq!(results[0].spent_cents, 900, "1200 − 300 退款");
    assert!(!results[0].over_budget);
}

#[test]
fn budget_progress_includes_child_category_transactions() {
    let conn = setup();
    insert_dummy_account(&conn);
    let parent_id = first_expense_category_id(&conn);
    let child_id = first_expense_subcategory_id(&conn, &parent_id);
    insert_budget(
        &conn,
        "budget-5",
        &parent_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    let fixture = vec![TxRow {
        id: "tx2",
        kind: TransactionKind::Expense,
        amount: 2000,
        category_id: child_id,
        date: "2026-07-10",
    }];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026-07"),
        "子分类交易应计入父分类预算"
    );
}

#[test]
fn budget_progress_over_budget() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-6", &cat_id, "monthly", 1000, "2026-07-01");
    let fixture = vec![TxRow {
        id: "tx5",
        kind: TransactionKind::Expense,
        amount: 2000,
        category_id: cat_id,
        date: "2026-07-10",
    }];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results[0].spent_cents, expense_net_sum(&fixture, "2026-07"));
    assert!(results[0].over_budget);
}

/// 月预算窗口 = 注入 today 所在自然月（与 start_date 无关）；上月不计入。
#[test]
fn budget_progress_only_counts_current_month() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-7", &cat_id, "monthly", 50000, "2026-07-01");
    let fixture = vec![
        TxRow {
            id: "tx6a",
            kind: TransactionKind::Expense,
            amount: 3000,
            category_id: cat_id.clone(),
            date: "2026-06-30", // 上月
        },
        TxRow {
            id: "tx6b",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: cat_id,
            date: "2026-07-02", // 当月
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026-07"),
        "月预算只计当前自然月"
    );
    assert_eq!(results[0].spent_cents, 1200);
}

/// 历史 start_date 的存量预算行按新规则滚动生效，不受旧日期影响（issue #182）。
#[test]
fn budget_progress_ignores_historical_start_date() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-9", &cat_id, "monthly", 50000, "2025-01-15");
    let fixture = vec![
        TxRow {
            id: "tx9a",
            kind: TransactionKind::Expense,
            amount: 3000,
            category_id: cat_id.clone(),
            date: "2025-01-20", // start_date 所在月，不得计入
        },
        TxRow {
            id: "tx9b",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: cat_id,
            date: "2026-07-02", // 当前自然月
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results[0].spent_cents, 800, "旧 start_date 不参与窗口");
}

/// 年预算窗口 = 注入 today 所在自然年全年累计（修复旧实现按月比对的口径 bug）。
#[test]
fn budget_progress_yearly_sums_whole_current_year() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-10", &cat_id, "yearly", 100000, "2026-03-01");
    let fixture = vec![
        TxRow {
            id: "tx10a",
            kind: TransactionKind::Expense,
            amount: 2000,
            category_id: cat_id.clone(),
            date: "2026-01-10", // 年初
        },
        TxRow {
            id: "tx10b",
            kind: TransactionKind::Refund,
            amount: 500,
            category_id: cat_id.clone(),
            date: "2026-03-10", // 年中退款冲减
        },
        TxRow {
            id: "tx10c",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: cat_id.clone(),
            date: "2026-07-02", // 当月
        },
        TxRow {
            id: "tx10d",
            kind: TransactionKind::Expense,
            amount: 5000,
            category_id: cat_id,
            date: "2025-12-31", // 去年，不计入
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026"),
        "年预算应为当前自然年全年累计"
    );
    assert_eq!(results[0].spent_cents, 2700, "2000 + 1200 − 500 退款");
}

#[test]
fn budget_progress_excludes_deleted() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-8", &cat_id, "monthly", 50000, "2026-07-01");
    insert_tx(
        &conn,
        &TxRow {
            id: "tx7",
            kind: TransactionKind::Expense,
            amount: 3000,
            category_id: cat_id,
            date: "2026-07-15",
        },
    );
    conn.execute("UPDATE transactions SET is_deleted=1 WHERE id='tx7'", [])
        .unwrap();
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results[0].spent_cents, 0);
}
