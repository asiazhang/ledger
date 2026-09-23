//! 储蓄目标域单测（spec #1750 / issue #1751，权威层——ADR-0087）：
//! 创建联动（自动建户、other 类型、1:1 绑定、金额码化守卫）与蓄水进度口径
//! （进度 = 专属账户余额，余额缓存、读时派生达成态）。
//!
//! 建库走统一测试数据库工厂（ADR-0084）；造数走公开写入口与工厂种子——进度
//! 变化必须由真实流水写入驱动（余额缓存由产品写路径维护，测试不直写缓存）。

use rusqlite::Connection;

use super::model::{SavingsGoalInput, SavingsGoalStatus};
use super::{create_savings_goal, list_savings_goal_progress};
use ledger_accounts::AccountType;
use ledger_transaction::{TransactionInput, TransactionKind};

fn setup() -> Connection {
    tauri_app_lib::test_support::open()
}

fn goal_input(name: &str, target_amount_cents: i64, deadline: Option<&str>) -> SavingsGoalInput {
    SavingsGoalInput {
        name: name.to_string(),
        target_amount_cents,
        deadline: deadline.map(str::to_string),
    }
}

fn only_progress(conn: &Connection) -> SavingsGoalProgressRow {
    let rows = list_savings_goal_progress(conn).expect("进度读取应成功");
    assert_eq!(rows.len(), 1, "夹具应只有一个目标");
    rows.into_iter().next().unwrap()
}

/// 进度行别名：断言处少一排泛型噪音。
type SavingsGoalProgressRow = super::model::SavingsGoalProgress;

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// 蓄水 = 真实转账（公开写入口，ADR-0086 纪律——余额缓存由产品写路径刷新）。
fn transfer(conn: &Connection, from: &str, to: &str, amount_cents: i64) {
    ledger_transaction::create(
        conn,
        TransactionInput {
            kind: TransactionKind::Transfer,
            amount_cents,
            currency_code: "CNY".into(),
            account_id: from.into(),
            to_account_id: Some(to.into()),
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-07-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
        },
    )
    .expect("转账应成功");
}

/// 取出 = 真实支出（同上，进度如实回落）。
fn spend(conn: &Connection, account_id: &str, amount_cents: i64) {
    ledger_transaction::create(
        conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-07-02".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
        },
    )
    .expect("支出应成功");
}

/// 创建联动（AC1）：目标创建成功 → 专属账户经账户域读命令可读（other 类型、
/// 与目标 1:1 绑定、币种 = 账本本位币）；两个目标各绑各的账户（无共享池）。
#[test]
fn create_goal_builds_bound_other_account() {
    let conn = setup();
    let goal_id = create_savings_goal(&conn, &goal_input("买车基金", 500_000, Some("2027-06-30")))
        .expect("创建目标应成功");

    let row = only_progress(&conn);
    assert_eq!(row.goal.id, goal_id);
    assert_eq!(row.goal.name, "买车基金");
    assert_eq!(row.goal.target_amount_cents, 500_000);
    assert_eq!(row.goal.deadline.as_deref(), Some("2027-06-30"));
    assert_eq!(row.goal.status, SavingsGoalStatus::Active);

    // 专属账户经账户域读命令可读：other 类型、与目标 1:1 绑定
    let accounts = ledger_accounts::list_accounts(&conn).expect("账户域读命令应成功");
    let bound: Vec<_> = accounts
        .iter()
        .filter(|a| a.id == row.goal.account_id)
        .collect();
    assert_eq!(bound.len(), 1, "目标绑定的账户应在账户域可读");
    let account = bound[0];
    assert_eq!(account.name, "买车基金");
    assert_eq!(account.kind, AccountType::Other);
    assert_eq!(
        account.currency_code, "CNY",
        "币种 = 账本本位币（缺省 CNY）"
    );

    // 1:1：第二个目标绑定第二个专属账户（共享资金池不在范围，spec #1750）
    create_savings_goal(&conn, &goal_input("教育金", 100_000, None)).expect("第二个目标创建应成功");
    let rows = list_savings_goal_progress(&conn).expect("进度读取应成功");
    assert_eq!(rows.len(), 2);
    let account_ids: std::collections::HashSet<&str> =
        rows.iter().map(|r| r.goal.account_id.as_str()).collect();
    assert_eq!(account_ids.len(), 2, "每个目标绑定各自专属账户");
}

/// 创建守卫（AC1）：目标金额非正数被码化错误拒绝，且零落库——无目标行、
/// 无专属账户（守卫先行 + 同一事务，失败不留任何中间态）。
#[test]
fn create_goal_rejects_non_positive_target_amount() {
    let conn = setup();
    let accounts_before = count(&conn, "SELECT COUNT(*) FROM accounts WHERE is_deleted=0");

    for amount in [0_i64, -1] {
        let err = create_savings_goal(&conn, &goal_input("负数目标", amount, None))
            .expect_err("非正目标金额应被拒绝");
        assert!(
            err.is_code("savings-goal.target-amount-positive"),
            "应报码化错误 savings-goal.target-amount-positive，实际 {err:?}"
        );
    }

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM goals WHERE is_deleted=0"),
        0,
        "拒绝创建不应留下目标行"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM accounts WHERE is_deleted=0"),
        accounts_before,
        "拒绝创建不应建出专属账户"
    );
}

/// 进度口径（AC2）：进度 = 专属账户余额（余额缓存）——初始已存 0、还差 = 目标额；
/// 转入上涨、达成 = 余额 ≥ 目标额、超额为带符号差值；支出如实回落、达成撤销。
#[test]
fn progress_follows_goal_account_balance() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_account_id = only_progress(&conn).goal.account_id;

    // 初始：已存 0、还差 = 目标额、未达成
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 0);
    assert_eq!(row.remaining_cents, 500_000);
    assert!(!row.achieved);
    assert_eq!(row.currency_code, "CNY");

    // 蓄水 = 真实转账：进度随之上涨
    transfer(&conn, "src", &goal_account_id, 200_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 200_000, "已存 = 专属账户余额");
    assert_eq!(row.remaining_cents, 300_000);
    assert!(!row.achieved);

    // 补足到目标额：达成态点亮、还差归零
    transfer(&conn, "src", &goal_account_id, 300_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 500_000);
    assert_eq!(row.remaining_cents, 0);
    assert!(row.achieved, "余额 ≥ 目标额即达成");

    // 超额存入：达成保持，还差为带符号负差值（不钳制、无百分比口径）
    transfer(&conn, "src", &goal_account_id, 50_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 550_000);
    assert_eq!(row.remaining_cents, -50_000);
    assert!(row.achieved);

    // 取出 = 真实支出：进度如实回落、达成撤销（无「扣回进度」特殊语义）
    spend(&conn, &goal_account_id, 100_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 450_000);
    assert_eq!(row.remaining_cents, 50_000);
    assert!(!row.achieved, "余额跌破目标额即退出达成态");
}
