//! 预算测试（issue #58 迁移后；issue #182 滚动窗口）：断言预算核心函数与
//! Amount 接缝的度量矩阵一致，不复制生产 SQL——期望值由 `signed_amount`
//! （kind × measure）对夹具逐行求和得出。窗口口径经注入 `today` 驱动。

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
    // 建库两行序经统一测试工厂承载（spec #728 / issue #754 / ADR-0084 决策 7）：
    // 工厂住根包，经 dev-dependency 测试环消费（#1101，ledger-transaction 同款）。
    tauri_app_lib::test_support::open()
}

/// 未知预算周期按 ADR-0050 码化（#1072）：闭集解析边界报稳定码与插值参数，
/// `message` 逐字保留；码本身经 `FromSql` 扁平化后不达前端（构造点口径，
/// 与 `account.type-unknown` 同形）。
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

/// 注入的「今天」：所有窗口口径测试共用，夹具日期围绕它铺开。
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

fn insert_dummy_account(conn: &Connection) {
    // 外键脚手架账户：工厂账户种子（归一签名，spec #728 / ADR-0084 决策 4）。
    tauri_app_lib::test_support::seed_account(conn, "dummy", "虚拟账户", "cash", "CNY", 0);
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
        rusqlite::params![r.id, r.kind.as_str(), r.amount, r.category_id, r.date, now, now, device_id(conn).unwrap()],
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
fn create_budget_and_list() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-1", &cat_id, "monthly", 50000, "2026-07-01");
    let budgets = list_budgets(&conn).unwrap();
    assert_eq!(budgets.len(), 1);
    assert_eq!(budgets[0].id, "budget-1");
}

/// 删除走公开入口 [`delete_budget`]（软删 + 审计字段 + op 产出同一事务；变异测试
/// 置换：本用例曾手写裸 SQL 模拟软删，不经被测函数，删除路径 no-op 变异全存活），
/// 读回经公开 [`list_budgets`] 断言已删行被排除。
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

// ---- create_budget：写入校验与同分类同周期查重（issue #183） ----

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
        let err = create_budget(&conn, &budget_input(&cat_id, None, amount)).unwrap_err();
        assert!(
            matches!(err, AppError::Coded { ref message, .. } if message.contains("预算金额必须为正数")),
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
    let err = create_budget(&conn, &budget_input(&cat_id, None, 1000)).unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message.contains("预算只能设置在支出分类上")),
        "{err:?}"
    );
    assert_eq!(budget_count(&conn), 0);
}

/// 不存在的分类同样拒绝（NotFound）。
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

/// 底线三：同分类同周期重复创建明确拒绝，且原预算数据不受影响。
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

/// 同分类不同周期共存（月预算 + 年预算互不冲突）。
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

/// 周期缺省为月度。
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

/// 软删后同分类同周期可重新创建（查重只看未删除行）。
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

// ---- update_budget：编辑金额（issue #184）/ 周期（issue #1946） ----

/// 编辑后金额生效，同步字段沿用软删除同一套机制：version +1、device_id 更新。
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

/// 编辑金额非正被拒绝（复用创建侧金额校验）。
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

/// 编辑不存在的预算（含已软删）被拒绝（NotFound）。
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

/// 预算的分类已删除时编辑被拒（复用创建侧分类校验）。
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

/// 编辑改周期生效：monthly → yearly 落库，版本机制同金额编辑（version+1，不新增行）。
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

/// period 为 None（旧载荷语义）时保持现周期不变。
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

/// 编辑改周期撞上同分类已有目标周期预算：复用创建侧同码拒绝，双方数据均不变。
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

/// 编辑改到自身现周期（同周期改金额）不触发唯一拒绝（排除自身）。
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

/// 同步载荷兼容（issue #1946）：存量 OpLog 旧载荷无 period 字段，反序列化语义
/// 不变（period = None，不改周期）。
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
    // 新载荷带 period 完整回读
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

/// 超标判定是严格大于（spent > limit）：恰好等于限额不算超标——边界断言使
/// `>` 边界变异（`>=`）可被击杀（变异测试报告的存活点，2026-10 首轮清理）。
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
    // 读回投影：在用预算行的 is_deleted 必须读出 false（from_row 布尔反转变异
    // 的击杀点，同上变异测试报告）。
    assert!(!results[0].budget.is_deleted);
    assert_eq!(results.len(), 1);
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

// ---- 年度汇总（issue #1948）：年度预算总额年化 + 本年已支去重 ----

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

/// 父子预算并存时同一笔支出只计一次：SUM 按交易逐行累计，不是按预算逐行相加
/// （ADR-0052 父子彼此独立，但汇总口径不得重复计数）。
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

/// 只设子分类预算：子分类自身的支出计入，父分类其他子树的支出不计入
/// （父分类没有预算，不构成覆盖）。
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

/// 未设预算的分类支出不进汇总（与年度预算总额「不回退实际支出」同一边界）。
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

/// 已支为 `expense_net`（退款冲减），与预算进度同口径；超标 = 已支 > 年化总额。
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
