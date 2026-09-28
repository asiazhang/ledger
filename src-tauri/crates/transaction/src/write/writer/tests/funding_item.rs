//! 出资项分解契约与子行落库（issue #1860 / ADR-0138）：normalize 的互斥、
//! kind 准入与逐条约束、子行随主行落库、refund 继承与显式覆盖。
//!
//! 断言模块外部行为：分解行主表账户落 NULL、子行按顺序位落库、余额缓存含出资端。
//! 全部基于内存库（test_support::open 已接线出资账户视图与余额刷新钩子）。

use ledger_infra::error::{AppError, ErrClass};
use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::funding_items::FundingItem;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};
use tauri_app_lib::test_support;

use super::common::input;

/// 分解条目构造器（测试速记）。
fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

/// 带分解的入参构造器：互斥要求 account_id 缺省。
fn funded_input(
    kind: TransactionKind,
    amount_cents: i64,
    funding: Vec<TransactionFundingInput>,
) -> Input {
    Input {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: None,
        funding: funding.iter().map(FundingItem::from).collect(),
        ..input(kind, amount_cents, "")
    }
}

// ---------------------------------------------------------------------------
// 契约互斥与必填（ADR-0138 决策 2）
// ---------------------------------------------------------------------------

/// 非空分解 + account_id → 码化互斥错误。
#[test]
fn normalize_rejects_funding_with_account_id() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            account_id: Some("acc-a".into()),
            ..funded_input(
                TransactionKind::Expense,
                1000,
                vec![item("acc-b", 1000, None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-account-conflict"));
}

/// 空分解且缺账户（非 refund）→ 码化必填错误（不用空串哨兵）。
#[test]
fn normalize_requires_account_when_no_funding() {
    let conn = test_support::open();
    let err = normalize(
        &conn,
        &Input {
            account_id: None,
            ..input(TransactionKind::Expense, 1000, "")
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.account-required"));
}

// ---------------------------------------------------------------------------
// kind 准入（ADR-0138 决策 3）
// ---------------------------------------------------------------------------

/// transfer 携带分解 → 码化拒绝（两端已表达，不做分解）。
#[test]
fn normalize_rejects_funding_on_transfer() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            to_account_id: Some("acc-a".into()),
            ..funded_input(
                TransactionKind::Transfer,
                1000,
                vec![item("acc-a", 1000, None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-item-unsupported"));
}

/// 投资 kind 携带分解 → 协议准入段拒绝（结算归因归出资账户，不混用）。
#[test]
fn create_rejects_funding_on_investment_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-inv", "acc-inv", "investment", "CNY", 0);
    let mut input = crate::tests::common::make_buy_input("acc-inv", "inst-guard", 1.0, 100, 0);
    input.funding = vec![item("acc-inv", 1, None)];
    let err = tauri_app_lib::ledger_transaction::create(&conn, input).unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-item-unsupported"));
}

// ---------------------------------------------------------------------------
// 逐条约束（ADR-0138 决策 4）
// ---------------------------------------------------------------------------

/// Σ 分解 ≠ 交易金额 → 码化错误（含两个插值）。
#[test]
fn normalize_rejects_sum_mismatch() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 2853, None), item("acc-b", 25307, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-sum-mismatch"));
    assert_eq!(
        err.to_string(),
        "出资分解合计（28160）与交易金额（1000）不一致"
    );
}

/// 分解条目金额非正 → 码化错误。
#[test]
fn normalize_rejects_non_positive_item_amount() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 1000, None), item("acc-a", 0, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-amount-positive"));
}

/// 标签超 50 字 → 码化错误；恰好 50 字通过。
#[test]
fn normalize_rejects_label_over_50_chars() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let label_51 = "标".repeat(51);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 1000, Some(&label_51))],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-label-too-long"));

    let label_50 = "标".repeat(50);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 1000, Some(&label_50))],
        ),
    )
    .unwrap();
    assert_eq!(norm.funding.len(), 1);
}

/// 出资账户不存在/已软删 → 码化 NotFound（准入与 account_id 同口径）。
#[test]
fn normalize_rejects_missing_funding_account() {
    let conn = test_support::open();
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("no-such-acc", 1000, None)],
        ),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

/// 出资账户币种与交易币种不一致 → 码化错误（不折算，现金腿不换币）。
#[test]
fn normalize_rejects_funding_currency_mismatch() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-usd", 1000, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("funding.currency-mismatch"));
}

/// 无账户类型闭集（ADR-0138 决策 4）：credit / debt 可出资（白条场景）。
#[test]
fn normalize_allows_credit_and_debt_accounts_as_funding() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-credit", "信用卡", "credit", "CNY", 0);
    test_support::seed_account(&conn, "acc-debt", "白条", "debt", "CNY", 0);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![
                item("acc-credit", 300, Some("定金")),
                item("acc-debt", 700, Some("尾款")),
            ],
        ),
    )
    .unwrap();
    assert_eq!(norm.account_id, None, "分解行主表账户列落 NULL");
    assert_eq!(norm.funding.len(), 2);
    assert_eq!(norm.funding[0].label.as_deref(), Some("定金"));
    assert_eq!(norm.funding[1].amount_cents, 700);
}

/// 同一账户多条出资项合法（定金 + 尾款靠标签区分，ADR-0138 决策 4）。
#[test]
fn normalize_allows_same_account_multiple_items() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Income,
            1000,
            vec![item("acc-a", 400, Some("定金")), item("acc-a", 600, None)],
        ),
    )
    .unwrap();
    assert_eq!(norm.funding.len(), 2);
}

// ---------------------------------------------------------------------------
// 子行落库与余额（ADR-0138 决策 1/7）
// ---------------------------------------------------------------------------

/// insert_row 落子行（顺序位随数组序），两账户余额按符号正确变动——组合支付
/// 样例：订单 ¥281.60 = 小金库 ¥28.53 + 余额 ¥253.07（ADR-0138 背景真实回单）。
#[test]
fn insert_row_persists_funding_rows_and_refreshes_balances() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-balance", "余额", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            28160,
            vec![
                item("acc-wallet", 2853, None),
                item("acc-balance", 25307, None),
            ],
        ),
    )
    .unwrap();
    let id = insert_row(&conn, &norm).unwrap();

    // 子行按顺序位落库：账户、金额、标签、顺序位逐项比对。
    let sort_account: Vec<(i64, String, i64)> = conn
        .prepare("SELECT sort, account_id, amount_cents FROM transaction_fundings WHERE transaction_id=?1 ORDER BY sort")
        .unwrap()
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        sort_account,
        vec![
            (0, "acc-wallet".into(), 2853),
            (1, "acc-balance".into(), 25307)
        ]
    );

    // 余额缓存（ADR-0067 机制）：出资端按 kind 符号（expense 记 −）整体重算。
    let cached = |id: &str| -> i64 {
        conn.query_row(
            "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(cached("acc-wallet"), -2853);
    assert_eq!(cached("acc-balance"), -25307);
}

// ---------------------------------------------------------------------------
// refund：继承、缺省派生前提与显式覆盖（ADR-0138 决策 5）
// ---------------------------------------------------------------------------

/// 落一笔分解 expense（refund 链测试的来源行）。
fn insert_funded_expense(conn: &rusqlite::Connection) -> String {
    let norm = normalize(
        conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-w1", 100, None), item("acc-b1", 900, None)],
        ),
    )
    .unwrap();
    insert_row(conn, &norm).unwrap()
}

#[test]
fn normalize_refund_inherits_null_account_from_funded_expense() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-w1", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b1", "余额", "cash", "CNY", 0);
    let source_id = insert_funded_expense(&conn);

    let norm = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 500, "")
        },
    )
    .unwrap();
    // 原支出为分解行：退款无单一账户可继承，主表账户落 NULL（读时走比例派生）。
    assert_eq!(norm.account_id, None);
    // 缺省（不携分解）不落子行——派生是读时推导、不落库。
    assert!(norm.funding.is_empty());
}

#[test]
fn normalize_refund_explicit_override_persists_breakdown() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-w1", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b1", "余额", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-x", "精准回退", "cash", "CNY", 0);
    let source_id = insert_funded_expense(&conn);

    let norm = normalize(
        &conn,
        &Input {
            account_id: None,
            refund_of_transaction_id: Some(source_id),
            funding: vec![FundingItem::from(&item("acc-x", 500, None))],
            ..input(TransactionKind::Refund, 500, "")
        },
    )
    .unwrap();
    // 显式覆盖与 expense/income 对称：分解行落 NULL + 子行随落库。
    assert_eq!(norm.account_id, None);
    assert_eq!(norm.funding.len(), 1);
    assert_eq!(norm.funding[0].account_id, "acc-x");
}
