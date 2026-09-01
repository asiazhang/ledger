//! `transaction::amount` 接缝的单元测试（issue #54 / spec #52）。
//!
//! 断言模块外部行为：kind 与 DB/wire 字符串边界的严格互转、kind→度量矩阵、
//! SQL 片段在真实内存库上与 Rust 助手聚合一致、本位币折算（基准为全局默认币种，
//! 独立于账户币种）。

use rusqlite::Connection;
use rusqlite::params;

use super::amount::*;

fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'cash',?3,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, id, currency],
    )
    .unwrap();
}

fn insert_txn(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    amount_native_cents: i64,
    account_id: &str,
    to_account_id: Option<&str>,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,'CNY',?3,?4,?5,'2026-02-01','2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![id, kind.as_str(), amount_native_cents, account_id, to_account_id],
    )
    .unwrap();
}

fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1',?1,?2,?3,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// TransactionKind 枚举
// ---------------------------------------------------------------------------

/// 全部 8 种 kind 与字符串互转严格往返。
#[test]
fn kind_string_roundtrip() {
    assert_eq!(TransactionKind::ALL.len(), 8);
    for kind in TransactionKind::ALL {
        assert_eq!(TransactionKind::parse(kind.as_str()).unwrap(), kind);
        assert_eq!(kind.to_string(), kind.as_str());
    }
}

/// serde 以小写字符串序列化（wire 兼容：与裸 String 同形），反序列化严格往返；
/// 未知值报错且文案与 parse 一致（中文）。
#[test]
fn kind_serde_roundtrip() {
    for kind in TransactionKind::ALL {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{}\"", kind.as_str()));
        let back: TransactionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
    let err = serde_json::from_str::<TransactionKind>("\"bonus\"").unwrap_err();
    assert!(err.to_string().contains("未知交易类型"), "实际: {err}");
}

/// 未知 kind 字符串应报错。
#[test]
fn kind_parse_rejects_unknown() {
    assert!(TransactionKind::parse("bonus").is_err());
    assert!(TransactionKind::parse("").is_err());
}

// ---------------------------------------------------------------------------
// kind→度量矩阵
// ---------------------------------------------------------------------------

/// 矩阵逐格断言：kind × 度量（含 transfer 双侧）。
/// 表值即 spec #52 的 kind→measure 矩阵，锁定语义不被悄悄改动。
#[test]
fn matrix_signed_amount_all_cells() {
    use Measure::*;
    use TransactionKind::*;
    let cells: &[(TransactionKind, Measure, i64)] = &[
        // account_flow（转出账户侧）
        (Income, AccountFlow(TransferSide::Out), 1),
        (Expense, AccountFlow(TransferSide::Out), -1),
        (Transfer, AccountFlow(TransferSide::Out), -1),
        (Refund, AccountFlow(TransferSide::Out), 1),
        (Buy, AccountFlow(TransferSide::Out), -1),
        (Sell, AccountFlow(TransferSide::Out), 1),
        (Dividend, AccountFlow(TransferSide::Out), 1),
        (Split, AccountFlow(TransferSide::Out), 0),
        // account_flow（转入账户侧）
        (Income, AccountFlow(TransferSide::In), 1),
        (Expense, AccountFlow(TransferSide::In), -1),
        (Transfer, AccountFlow(TransferSide::In), 1),
        (Refund, AccountFlow(TransferSide::In), 1),
        (Buy, AccountFlow(TransferSide::In), -1),
        (Sell, AccountFlow(TransferSide::In), 1),
        (Dividend, AccountFlow(TransferSide::In), 1),
        (Split, AccountFlow(TransferSide::In), 0),
        // expense_net：毛支出 − 退款；投资类不计入
        (Income, ExpenseNet, 0),
        (Expense, ExpenseNet, 1),
        (Transfer, ExpenseNet, 0),
        (Refund, ExpenseNet, -1),
        (Buy, ExpenseNet, 0),
        (Sell, ExpenseNet, 0),
        (Dividend, ExpenseNet, 0),
        (Split, ExpenseNet, 0),
        // income_net：收入 + 分红
        (Income, IncomeNet, 1),
        (Expense, IncomeNet, 0),
        (Transfer, IncomeNet, 0),
        (Refund, IncomeNet, 0),
        (Buy, IncomeNet, 0),
        (Sell, IncomeNet, 0),
        (Dividend, IncomeNet, 1),
        (Split, IncomeNet, 0),
        // refund_gross：仅退款
        (Income, RefundGross, 0),
        (Expense, RefundGross, 0),
        (Transfer, RefundGross, 0),
        (Refund, RefundGross, 1),
        (Buy, RefundGross, 0),
        (Sell, RefundGross, 0),
        (Dividend, RefundGross, 0),
        (Split, RefundGross, 0),
        // policy_premium：仅挂单保费（expense），无退款冲减（ADR-0051 决策 4）
        (Income, PolicyPremium, 0),
        (Expense, PolicyPremium, 1),
        (Transfer, PolicyPremium, 0),
        (Refund, PolicyPremium, 0),
        (Buy, PolicyPremium, 0),
        (Sell, PolicyPremium, 0),
        (Dividend, PolicyPremium, 0),
        (Split, PolicyPremium, 0),
        // policy_inflow：仅挂单现金流入（income）
        (Income, PolicyInflow, 1),
        (Expense, PolicyInflow, 0),
        (Transfer, PolicyInflow, 0),
        (Refund, PolicyInflow, 0),
        (Buy, PolicyInflow, 0),
        (Sell, PolicyInflow, 0),
        (Dividend, PolicyInflow, 0),
        (Split, PolicyInflow, 0),
    ];
    for &(kind, measure, expect_sign) in cells {
        assert_eq!(
            signed_amount(kind, 700, measure),
            expect_sign * 700,
            "kind={kind:?} measure={measure:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// SQL 片段 builder ↔ Rust 助手一致性
// ---------------------------------------------------------------------------

fn sql_sum(conn: &Connection, expr: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COALESCE(SUM({expr}),0) FROM transactions t WHERE t.is_deleted=0"),
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn rust_sum(conn: &Connection, measure: Measure) -> i64 {
    // 与 insert_txn 的 kind/amount 布局耦合：按写入顺序读回全部行。
    // kind 列经 FromSql 直读为枚举（DB 边界映射，与生产路径一致）。
    let mut stmt = conn
        .prepare("SELECT kind, amount_native_cents FROM transactions WHERE is_deleted=0")
        .unwrap();
    let rows: Vec<(TransactionKind, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows.into_iter()
        .map(|(k, amt)| signed_amount(k, amt, measure))
        .sum()
}

/// 每种 kind 各写一行（金额互异防串位），四个度量的 SQL 片段聚合
/// 必须与 Rust `signed_amount` 逐行求和一致。
#[test]
fn sql_exprs_match_rust_sums() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    for (i, kind) in TransactionKind::ALL.into_iter().enumerate() {
        insert_txn(
            &conn,
            &format!("t-{i}"),
            kind,
            100 + i as i64 * 7,
            "acc",
            None,
        );
    }

    let measures = [
        Measure::AccountFlow(TransferSide::Out),
        Measure::AccountFlow(TransferSide::In),
        Measure::ExpenseNet,
        Measure::IncomeNet,
        Measure::RefundGross,
        Measure::PolicyPremium,
        Measure::PolicyInflow,
    ];
    for measure in measures {
        let expr = match measure {
            Measure::AccountFlow(side) => account_flow_expr("t", side),
            Measure::ExpenseNet => expense_net_expr("t"),
            Measure::IncomeNet => income_net_expr("t"),
            Measure::RefundGross => refund_gross_expr("t"),
            Measure::PolicyPremium => policy_premium_expr("t"),
            Measure::PolicyInflow => policy_inflow_expr("t"),
        };
        assert_eq!(
            sql_sum(&conn, &expr),
            rust_sum(&conn, measure),
            "SQL 片段与 Rust 聚合不一致: {measure:?} => {expr}"
        );
    }
}

/// 毛支出恒等式（issue #57）：`expense_gross = expense_net + refund_gross`，
/// SQL 片段在真实库上的聚合必须与两侧度量之和逐分一致。
#[test]
fn expense_gross_expr_equals_net_plus_refund_gross() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    for (i, kind) in TransactionKind::ALL.into_iter().enumerate() {
        insert_txn(
            &conn,
            &format!("t-{i}"),
            kind,
            100 + i as i64 * 7,
            "acc",
            None,
        );
    }
    assert_eq!(
        sql_sum(&conn, &expense_gross_expr("t")),
        sql_sum(&conn, &expense_net_expr("t")) + sql_sum(&conn, &refund_gross_expr("t")),
        "毛支出恒等式在真实库上应逐分成立"
    );
}

/// 参与度量聚合的 kind 清单由矩阵导出：仅系数非 0 的 kind 入列，
/// 与 kind→度量矩阵单一真源保持同步（income_net 必须含 dividend）。
#[test]
fn contributing_kinds_follow_matrix() {
    assert_eq!(
        contributing_kinds(Measure::ExpenseNet),
        vec!["expense", "refund"]
    );
    assert_eq!(
        contributing_kinds(Measure::IncomeNet),
        vec!["income", "dividend"]
    );
    assert_eq!(contributing_kinds(Measure::RefundGross), vec!["refund"]);
    assert_eq!(contributing_kinds(Measure::PolicyPremium), vec!["expense"]);
    assert_eq!(contributing_kinds(Measure::PolicyInflow), vec!["income"]);
    assert_eq!(
        contributing_kinds(Measure::AccountFlow(TransferSide::Out)),
        vec![
            "income", "expense", "transfer", "refund", "buy", "sell", "dividend"
        ]
    );
    assert_eq!(
        contributing_kinds(Measure::AccountFlow(TransferSide::In)),
        contributing_kinds(Measure::AccountFlow(TransferSide::Out))
    );
}

/// account_flow 片段按「转出侧 join account_id / 转入侧 join to_account_id」
/// 组合出的账户余额，与 Rust 助手按账户过滤求和一致。
#[test]
fn account_flow_expr_balances_match_rust() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");

    // acc-a：收入 5000、支出 1200、退款 300、买入 2000、拆股 0、转出 800 到 acc-b
    insert_txn(&conn, "t1", TransactionKind::Income, 5000, "acc-a", None);
    insert_txn(&conn, "t2", TransactionKind::Expense, 1200, "acc-a", None);
    insert_txn(&conn, "t3", TransactionKind::Refund, 300, "acc-a", None);
    insert_txn(&conn, "t4", TransactionKind::Buy, 2000, "acc-a", None);
    insert_txn(&conn, "t5", TransactionKind::Split, 9999, "acc-a", None);
    insert_txn(
        &conn,
        "t6",
        TransactionKind::Transfer,
        800,
        "acc-a",
        Some("acc-b"),
    );
    // acc-b：分红 60
    insert_txn(&conn, "t7", TransactionKind::Dividend, 60, "acc-b", None);

    let balance_sql = |account: &str| -> i64 {
        let out: i64 = conn
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM({}),0) FROM transactions t \
                     WHERE t.is_deleted=0 AND t.account_id=?1",
                    account_flow_expr("t", TransferSide::Out)
                ),
                params![account],
                |r| r.get(0),
            )
            .unwrap();
        let incoming: i64 = conn
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM({}),0) FROM transactions t \
                     WHERE t.is_deleted=0 AND t.to_account_id=?1",
                    account_flow_expr("t", TransferSide::In)
                ),
                params![account],
                |r| r.get(0),
            )
            .unwrap();
        out + incoming
    };

    let balance_rust = |account: &str| -> i64 {
        let mut stmt = conn
            .prepare(
                "SELECT kind, amount_native_cents, account_id, to_account_id \
                 FROM transactions WHERE is_deleted=0",
            )
            .unwrap();
        let rows: Vec<(TransactionKind, i64, String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows.into_iter()
            .filter(|(_, _, acc, to)| {
                acc == account || (to.is_some() && to.as_deref() == Some(account))
            })
            .map(|(kind, amt, acc, _to)| {
                let side = if acc == account {
                    TransferSide::Out
                } else {
                    TransferSide::In
                };
                signed_amount(kind, amt, Measure::AccountFlow(side))
            })
            .sum()
    };

    // 期望：acc-a = +5000 −1200 +300 −2000 +0 −800 = 1300；acc-b = +800 +60 = 860
    assert_eq!(balance_rust("acc-a"), 1300);
    assert_eq!(balance_rust("acc-b"), 860);
    assert_eq!(balance_sql("acc-a"), balance_rust("acc-a"));
    assert_eq!(balance_sql("acc-b"), balance_rust("acc-b"));
}

// ---------------------------------------------------------------------------
// convert_to_native
// ---------------------------------------------------------------------------

/// 币种与默认币种相同 → 1:1 原样返回。
#[test]
fn convert_to_native_same_currency_is_identity() {
    let conn = setup_db();
    assert_eq!(
        convert_to_native(&conn, 12345, default_currency_code()).unwrap(),
        12345
    );
}

/// 非默认币种按汇率折算到全局默认币种。
#[test]
fn convert_to_native_uses_rate_to_default_currency() {
    let conn = setup_db();
    insert_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(convert_to_native(&conn, 10000, "USD").unwrap(), 72000);
}

/// 折算基准是全局默认币种，与账户币种无关：
/// 即使存在 USD 账户，USD 金额仍折算到 CNY，而非 1:1 落库。
#[test]
fn convert_to_native_is_independent_of_account_currency() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(convert_to_native(&conn, 10000, "USD").unwrap(), 72000);
}

/// 只有反向汇率时取倒数折算。
#[test]
fn convert_to_native_uses_reverse_rate_when_only_reverse_exists() {
    let conn = setup_db();
    insert_rate(&conn, "CNY", "EUR", 0.13);
    // 1 EUR = 1/0.13 CNY ≈ 7.6923
    assert_eq!(convert_to_native(&conn, 10000, "EUR").unwrap(), 76923);
}

/// 正反向汇率均无 → 报错（不允许静默 1:1 混币种相加）。
#[test]
fn convert_to_native_errors_without_rate() {
    let conn = setup_db();
    assert!(convert_to_native(&conn, 10000, "JPY").is_err());
}

/// 非正汇率（正查或反查）应报错，不得静默产出 0/负本位币金额。
#[test]
fn convert_to_native_rejects_non_positive_rate() {
    let conn = setup_db();
    insert_rate(&conn, "USD", "CNY", 0.0);
    assert!(convert_to_native(&conn, 10000, "USD").is_err());

    let conn = setup_db();
    insert_rate(&conn, "CNY", "EUR", -0.13);
    assert!(convert_to_native(&conn, 10000, "EUR").is_err());
}
