use chrono::NaiveDate;
use rusqlite::Connection;

use super::model::{BudgetInput, BudgetPeriod};
use crate::{
    BudgetCommand, annual_budget_total_cents, budget_annual_summary, budget_progress_rows,
    create_budget, delete_budget, list_budgets, update_budget,
};
use ledger_infra::db::now_iso;
use ledger_infra::error::{AppError, ErrClass};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::amount::{Measure, TransactionKind, signed_amount};

fn setup() -> Connection {
    tauri_app_lib::test_support::open()
}

#[test]
fn budget_period_parse_rejects_unknown_with_code() {
    let err = "quarterly".parse::<BudgetPeriod>().unwrap_err();
    let wire = serde_json::to_value(&err).unwrap();
    assert_eq!(wire["kind"], "Invalid");
    assert_eq!(wire["code"], "budget.period-unknown");
    assert_eq!(wire["params"], serde_json::json!(["quarterly"]));
    assert_eq!(err.to_string(), "未知预算周期: quarterly");
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
        rusqlite::params![id, category_id, period, amount_cents, start_date, now, now, 1, device_id(conn).unwrap()],
    ).unwrap();
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

fn insert_dummy_account(conn: &Connection) {
    tauri_app_lib::test_support::seed_account(conn, "dummy", "虚拟账户", "cash", "CNY", 0);
}

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
        rusqlite::params![r.id, r.kind.as_str(), r.amount, r.category_id, r.date, now, now, device_id(conn).unwrap()],
    )
    .unwrap();
}

fn expense_net_sum(rows: &[TxRow], prefix: &str) -> i64 {
    rows.iter()
        .filter(|r| r.date.starts_with(prefix))
        .map(|r| signed_amount(r.kind, r.amount, Measure::ExpenseNet))
        .sum()
}

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

#[test]
fn create_budget_and_list() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-1", &cat_id, "monthly", 50000, "2026-07-01");
    let budgets = list_budgets(&conn).unwrap();
    assert_eq!(budgets.len(), 1);
    assert_eq!(budgets[0].id, "budget-1");
}

#[test]
fn delete_budget_soft_deletes() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    create_budget(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Monthly), 50000),
    )
    .unwrap();
    let budgets = list_budgets(&conn).unwrap();
    assert_eq!(budgets.len(), 1);
    assert!(
        !budgets[0].is_deleted,
        "在用行的 is_deleted 投影必须读出 false"
    );
    let id = budgets[0].id.clone();

    delete_budget(&conn, &id).unwrap();

    assert!(
        list_budgets(&conn).unwrap().is_empty(),
        "软删后公开读回不得再含该行"
    );
}

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

#[test]
fn create_budget_rejects_non_positive_amount() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    for amount in [0, -100] {
        let err = create_budget(&conn, &budget_input(&cat_id, None, amount)).unwrap_err();
        assert!(
            matches!(err, AppError::Coded { ref message, .. } if message.contains("预算金额必须为正数")),
            "金额 {amount} 应被拒绝: {err:?}"
        );
    }
    assert_eq!(budget_count(&conn), 0);
}

#[test]
fn create_budget_rejects_income_category() {
    let conn = setup();
    let cat_id = first_income_category_id(&conn);
    let err = create_budget(&conn, &budget_input(&cat_id, None, 1000)).unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message.contains("预算只能设置在支出分类上")),
        "{err:?}"
    );
    assert_eq!(budget_count(&conn), 0);
}

#[test]
fn create_budget_rejects_missing_category() {
    let conn = setup();
    let err = create_budget(&conn, &budget_input("no-such-cat", None, 1000)).unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn create_budget_rejects_duplicate_monthly_and_keeps_original() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-dup", &cat_id, "monthly", 50000, "2026-07-01");
    let err = create_budget(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Monthly), 1000),
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message == "该分类已存在按月预算，可编辑该预算的金额"),
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
    let err = create_budget(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Yearly), 1000),
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message == "该分类已存在按年预算，可编辑该预算的金额"),
        "{err:?}"
    );
}

#[test]
fn create_budget_allows_same_category_different_period() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-m", &cat_id, "monthly", 50000, "2026-07-01");
    let id = create_budget(
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

#[test]
fn create_budget_defaults_to_monthly() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    let id = create_budget(&conn, &budget_input(&cat_id, None, 1000)).unwrap();
    let period: String = conn
        .query_row("SELECT period FROM budgets WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(period, "monthly");
}

#[test]
fn create_budget_allows_recreate_after_soft_delete() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-old", &cat_id, "monthly", 50000, "2026-07-01");
    conn.execute("UPDATE budgets SET is_deleted=1 WHERE id='budget-old'", [])
        .unwrap();
    create_budget(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Monthly), 1000),
    )
    .unwrap();
    assert_eq!(budget_count(&conn), 2);
}

#[test]
fn update_budget_changes_amount_and_bumps_version() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-edit",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    update_budget(&conn, "budget-edit", 80000, None).unwrap();
    let (amount, version): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents,version FROM budgets WHERE id='budget-edit'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount, 80000);
    assert_eq!(version, 2, "编辑应 version+1");
    assert_eq!(budget_count(&conn), 1, "编辑不得新增行");
}

#[test]
fn update_budget_rejects_non_positive_amount() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-edit2",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    for amount in [0, -100] {
        let err = update_budget(&conn, "budget-edit2", amount, None).unwrap_err();
        assert!(
            matches!(err, AppError::Coded { ref message, .. } if message.contains("预算金额必须为正数")),
            "金额 {amount} 应被拒绝: {err:?}"
        );
    }
    let amount: i64 = conn
        .query_row(
            "SELECT amount_cents FROM budgets WHERE id='budget-edit2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, 50000, "被拒编辑不得改动原金额");
}

#[test]
fn update_budget_rejects_missing_or_deleted_budget() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-gone",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    conn.execute("UPDATE budgets SET is_deleted=1 WHERE id='budget-gone'", [])
        .unwrap();
    let err = update_budget(&conn, "budget-gone", 1000, None).unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
    let err = update_budget(&conn, "no-such-budget", 1000, None).unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn update_budget_rejects_when_category_deleted() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-orphan",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    conn.execute(
        "UPDATE categories SET is_deleted=1 WHERE id=?1",
        rusqlite::params![cat_id],
    )
    .unwrap();
    let err = update_budget(&conn, "budget-orphan", 1000, None).unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn update_budget_changes_period() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-period",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    update_budget(&conn, "budget-period", 60000, Some(BudgetPeriod::Yearly)).unwrap();
    let (period, amount, version): (String, i64, i64) = conn
        .query_row(
            "SELECT period,amount_cents,version FROM budgets WHERE id='budget-period'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(period, "yearly");
    assert_eq!(amount, 60000);
    assert_eq!(version, 2, "编辑应 version+1");
    assert_eq!(budget_count(&conn), 1, "编辑不得新增行");
}

#[test]
fn update_budget_none_period_keeps_current_period() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-keep",
        &cat_id,
        "yearly",
        100000,
        "2026-01-01",
    );
    update_budget(&conn, "budget-keep", 120000, None).unwrap();
    let (period, amount): (String, i64) = conn
        .query_row(
            "SELECT period,amount_cents FROM budgets WHERE id='budget-keep'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(period, "yearly", "None 应保持现周期");
    assert_eq!(amount, 120000);
}

#[test]
fn update_budget_rejects_conflicting_period_and_keeps_rows() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-conflict-m",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    insert_budget(
        &conn,
        "budget-conflict-y",
        &cat_id,
        "yearly",
        100000,
        "2026-01-01",
    );
    let err = update_budget(
        &conn,
        "budget-conflict-m",
        50000,
        Some(BudgetPeriod::Yearly),
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message == "该分类已存在按年预算，可编辑该预算的金额"),
        "{err:?}"
    );
    let rows: Vec<(String, i64)> = conn
        .prepare(
            "SELECT period,amount_cents FROM budgets WHERE id LIKE 'budget-conflict-%' ORDER BY id",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![("monthly".into(), 50000), ("yearly".into(), 100000)],
        "被拒编辑不得改动任何一行"
    );
}

#[test]
fn update_budget_same_period_excludes_self() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(
        &conn,
        "budget-self",
        &cat_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    update_budget(&conn, "budget-self", 70000, Some(BudgetPeriod::Monthly)).unwrap();
    let amount: i64 = conn
        .query_row(
            "SELECT amount_cents FROM budgets WHERE id='budget-self'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, 70000);
}

#[test]
fn update_command_old_payload_without_period_deserializes_as_none() {
    let cmd: BudgetCommand =
        serde_json::from_str(r#"{"action":"update","id":"b1","amount_cents":100}"#).unwrap();
    match cmd {
        BudgetCommand::Update {
            id,
            amount_cents,
            period,
        } => {
            assert_eq!(id, "b1");
            assert_eq!(amount_cents, 100);
            assert_eq!(period, None, "旧载荷缺 period 字段应反序列化为 None");
        }
        other => panic!("应为 Update 命令: {other:?}"),
    }
    let cmd: BudgetCommand = serde_json::from_str(
        r#"{"action":"update","id":"b1","amount_cents":100,"period":"yearly"}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        BudgetCommand::Update {
            period: Some(BudgetPeriod::Yearly),
            ..
        }
    ));
}

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

#[test]
fn budget_progress_at_limit_is_not_over_budget() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    create_budget(
        &conn,
        &budget_input(&cat_id, Some(BudgetPeriod::Monthly), 2000),
    )
    .unwrap();
    let fixture = vec![TxRow {
        id: "tx-limit",
        kind: TransactionKind::Expense,
        amount: 2000,
        category_id: cat_id,
        date: "2026-07-10",
    }];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results[0].spent_cents, 2000);
    assert!(!results[0].over_budget, "恰好等于限额不算超标（严格大于）");
    assert!(!results[0].budget.is_deleted);
    assert_eq!(results.len(), 1);
}

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
            date: "2026-06-30",
        },
        TxRow {
            id: "tx6b",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: cat_id,
            date: "2026-07-02",
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
            date: "2025-01-20",
        },
        TxRow {
            id: "tx9b",
            kind: TransactionKind::Expense,
            amount: 800,
            category_id: cat_id,
            date: "2026-07-02",
        },
    ];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn, today()).unwrap();
    assert_eq!(results[0].spent_cents, 800, "旧 start_date 不参与窗口");
}

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
            date: "2026-01-10",
        },
        TxRow {
            id: "tx10b",
            kind: TransactionKind::Refund,
            amount: 500,
            category_id: cat_id.clone(),
            date: "2026-03-10",
        },
        TxRow {
            id: "tx10c",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: cat_id.clone(),
            date: "2026-07-02",
        },
        TxRow {
            id: "tx10d",
            kind: TransactionKind::Expense,
            amount: 5000,
            category_id: cat_id,
            date: "2025-12-31",
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

fn other_top_expense_category_id(conn: &Connection, exclude: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE kind='expense' AND parent_id IS NULL AND id<>?1 \
         ORDER BY created_at LIMIT 1",
        rusqlite::params![exclude],
        |r| r.get(0),
    )
    .unwrap()
}

fn insert_expense_tx(
    conn: &Connection,
    id: &'static str,
    category_id: &str,
    amount: i64,
    date: &'static str,
) {
    insert_tx(
        conn,
        &TxRow {
            id,
            kind: TransactionKind::Expense,
            amount,
            category_id: category_id.to_string(),
            date,
        },
    );
}

#[test]
fn annual_budget_total_annualizes_monthly_and_adds_yearly() {
    let conn = setup();
    let parent_id = first_expense_category_id(&conn);
    let child_id = first_expense_subcategory_id(&conn, &parent_id);
    insert_budget(
        &conn,
        "budget-a1",
        &parent_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    insert_budget(
        &conn,
        "budget-a2",
        &child_id,
        "yearly",
        120000,
        "2026-07-01",
    );
    assert_eq!(
        annual_budget_total_cents(&conn).unwrap(),
        50000 * 12 + 120000,
        "月预算 ×12 年化 + 年预算直接计入"
    );
}

#[test]
fn annual_summary_zero_without_budgets() {
    let conn = setup();
    let summary = budget_annual_summary(&conn, today()).unwrap();
    assert_eq!(summary.annual_budget_total_cents, 0);
    assert_eq!(summary.spent_cents, 0);
    assert!(!summary.over_budget);
}

#[test]
fn annual_summary_counts_each_transaction_once_with_parent_and_child_budgets() {
    let conn = setup();
    insert_dummy_account(&conn);
    let parent_id = first_expense_category_id(&conn);
    let child_id = first_expense_subcategory_id(&conn, &parent_id);
    insert_budget(
        &conn,
        "budget-b1",
        &parent_id,
        "monthly",
        50000,
        "2026-07-01",
    );
    insert_budget(
        &conn,
        "budget-b2",
        &child_id,
        "monthly",
        10000,
        "2026-07-01",
    );
    insert_expense_tx(&conn, "tx-b1", &child_id, 2000, "2026-03-04");
    let summary = budget_annual_summary(&conn, today()).unwrap();
    assert_eq!(summary.spent_cents, 2000, "父预算与子预算并存只计一次");
}

#[test]
fn annual_summary_counts_child_spending_only_for_child_budget() {
    let conn = setup();
    insert_dummy_account(&conn);
    let parent_id = first_expense_category_id(&conn);
    let child_id = first_expense_subcategory_id(&conn, &parent_id);
    insert_budget(&conn, "budget-c1", &child_id, "yearly", 10000, "2026-07-01");
    insert_expense_tx(&conn, "tx-c1", &child_id, 1500, "2026-05-05");
    insert_expense_tx(&conn, "tx-c2", &parent_id, 900, "2026-05-06");
    let summary = budget_annual_summary(&conn, today()).unwrap();
    assert_eq!(summary.spent_cents, 1500, "父分类自身支出不进子分类预算");
}

#[test]
fn annual_summary_excludes_categories_without_budget() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    let other_id = other_top_expense_category_id(&conn, &cat_id);
    insert_budget(&conn, "budget-d1", &cat_id, "monthly", 50000, "2026-07-01");
    insert_expense_tx(&conn, "tx-d1", &other_id, 3000, "2026-04-01");
    let summary = budget_annual_summary(&conn, today()).unwrap();
    assert_eq!(summary.spent_cents, 0, "未设预算分类的支出不进汇总");
}

#[test]
fn annual_summary_window_is_current_natural_year() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-e1", &cat_id, "yearly", 100000, "2026-07-01");
    insert_expense_tx(&conn, "tx-e1", &cat_id, 2000, "2025-12-31");
    insert_expense_tx(&conn, "tx-e2", &cat_id, 1500, "2026-01-02");
    let summary = budget_annual_summary(&conn, today()).unwrap();
    assert_eq!(summary.spent_cents, 1500, "只累计 today 所在自然年");
}

#[test]
fn annual_summary_uses_expense_net_and_flags_over_budget() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-f1", &cat_id, "yearly", 1000, "2026-07-01");
    insert_expense_tx(&conn, "tx-f1", &cat_id, 2000, "2026-02-02");
    insert_tx(
        &conn,
        &TxRow {
            id: "tx-f2",
            kind: TransactionKind::Refund,
            amount: 600,
            category_id: cat_id.clone(),
            date: "2026-02-03",
        },
    );
    let summary = budget_annual_summary(&conn, today()).unwrap();
    assert_eq!(summary.spent_cents, 1400, "2000 − 600 退款");
    assert_eq!(summary.annual_budget_total_cents, 1000);
    assert!(summary.over_budget);
}
