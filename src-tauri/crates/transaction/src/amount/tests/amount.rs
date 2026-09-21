//! `transaction::amount` 接缝的单元测试（issue #54 / spec #52）。
//!
//! 断言模块外部行为：kind 与 DB/wire 字符串边界的严格互转、kind→度量矩阵、
//! SQL 片段在真实内存库上与 Rust 助手聚合一致、本位币折算（基准为全局默认币种，
//! 独立于账户币种）。

use ledger_infra::error::AppError;
use rusqlite::Connection;
use rusqlite::params;

use tauri_app_lib::ledger_transaction::amount::*;
use tauri_app_lib::test_support;

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

// ---------------------------------------------------------------------------
// TransactionKind 枚举
// ---------------------------------------------------------------------------

/// 全部 9 种 kind 与字符串互转严格往返。
#[test]
fn kind_string_roundtrip() {
    assert_eq!(TransactionKind::ALL.len(), 9);
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

/// 未知 kind 字符串应报错：按 ADR-0050 码化（稳定 `code` + 插值参数），
/// `message` 逐字不变、合法值清单仍由宏同一批字面量同源生成。
#[test]
fn kind_parse_rejects_unknown() {
    let err = TransactionKind::parse("bonus").unwrap_err();
    assert_eq!(err.code(), Some("transaction.kind-unknown"));
    assert_eq!(
        err.to_string(),
        "未知交易类型: bonus（合法值: income/expense/transfer/refund/buy/sell/dividend/split/convert）",
        "message 逐字不变（ADR-0050 只增不改）"
    );
    assert_eq!(
        serde_json::to_value(&err).unwrap()["params"],
        serde_json::json!([
            "bonus",
            "income/expense/transfer/refund/buy/sell/dividend/split/convert"
        ]),
        "params 按动态值出现顺序：未知值 → 同源合法值清单"
    );
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
        (Convert, AccountFlow(TransferSide::Out), 0),
        // account_flow（转入账户侧）
        (Income, AccountFlow(TransferSide::In), 1),
        (Expense, AccountFlow(TransferSide::In), -1),
        (Transfer, AccountFlow(TransferSide::In), 1),
        (Refund, AccountFlow(TransferSide::In), 1),
        (Buy, AccountFlow(TransferSide::In), -1),
        (Sell, AccountFlow(TransferSide::In), 1),
        (Dividend, AccountFlow(TransferSide::In), 1),
        (Split, AccountFlow(TransferSide::In), 0),
        (Convert, AccountFlow(TransferSide::In), 0),
        // expense_net：毛支出 − 退款；投资类不计入
        (Income, ExpenseNet, 0),
        (Expense, ExpenseNet, 1),
        (Transfer, ExpenseNet, 0),
        (Refund, ExpenseNet, -1),
        (Buy, ExpenseNet, 0),
        (Sell, ExpenseNet, 0),
        (Dividend, ExpenseNet, 0),
        (Split, ExpenseNet, 0),
        (Convert, ExpenseNet, 0),
        // income_net：收入 + 分红
        (Income, IncomeNet, 1),
        (Expense, IncomeNet, 0),
        (Transfer, IncomeNet, 0),
        (Refund, IncomeNet, 0),
        (Buy, IncomeNet, 0),
        (Sell, IncomeNet, 0),
        (Dividend, IncomeNet, 1),
        (Split, IncomeNet, 0),
        (Convert, IncomeNet, 0),
        // refund_gross：仅退款
        (Income, RefundGross, 0),
        (Expense, RefundGross, 0),
        (Transfer, RefundGross, 0),
        (Refund, RefundGross, 1),
        (Buy, RefundGross, 0),
        (Sell, RefundGross, 0),
        (Dividend, RefundGross, 0),
        (Split, RefundGross, 0),
        (Convert, RefundGross, 0),
        // policy_premium：仅挂单保费（expense），无退款冲减（ADR-0051 决策 4）
        (Income, PolicyPremium, 0),
        (Expense, PolicyPremium, 1),
        (Transfer, PolicyPremium, 0),
        (Refund, PolicyPremium, 0),
        (Buy, PolicyPremium, 0),
        (Sell, PolicyPremium, 0),
        (Dividend, PolicyPremium, 0),
        (Split, PolicyPremium, 0),
        (Convert, PolicyPremium, 0),
        // policy_inflow：仅挂单现金流入（income）
        (Income, PolicyInflow, 1),
        (Expense, PolicyInflow, 0),
        (Transfer, PolicyInflow, 0),
        (Refund, PolicyInflow, 0),
        (Buy, PolicyInflow, 0),
        (Sell, PolicyInflow, 0),
        (Dividend, PolicyInflow, 0),
        (Split, PolicyInflow, 0),
        (Convert, PolicyInflow, 0),
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);

    // acc-a：收入 5000、支出 1200、退款 300、买入 2000、拆股 0、转出 800 到 acc-b
    insert_txn(&conn, "t1", TransactionKind::Income, 5000, "acc-a", None);
    insert_txn(&conn, "t2", TransactionKind::Expense, 1200, "acc-a", None);
    insert_txn(&conn, "t3", TransactionKind::Refund, 300, "acc-a", None);
    insert_txn(&conn, "t4", TransactionKind::Buy, 2000, "acc-a", None);
    insert_txn(&conn, "t5", TransactionKind::Split, 9999, "acc-a", None);
    // 基金转换（ADR-0099）：两腿同记录、无现金腿——金额非 0 也不入任何账户余额。
    insert_txn(&conn, "t5c", TransactionKind::Convert, 4321, "acc-a", None);
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
// convert_to_native_current
// ---------------------------------------------------------------------------

/// 币种与默认币种相同 → 1:1 原样返回。
#[test]
fn convert_to_native_current_same_currency_is_identity() {
    let conn = test_support::open();
    assert_eq!(
        convert_to_native_current(&conn, 12345, &default_currency_code(&conn).unwrap()).unwrap(),
        12345
    );
}

/// 本位币基准读账本级设置（issue #858 / ADR-0091 决策 3）：缺 key 回默认 CNY，
/// 设置后随设置切换（存储与写协议归币种域，折算基准单一根据）。
#[test]
fn default_currency_code_reads_ledger_setting() {
    let conn = test_support::open();
    assert_eq!(default_currency_code(&conn).unwrap(), "CNY");
    ledger_currencies::set_base_currency(&conn, "USD").unwrap();
    assert_eq!(default_currency_code(&conn).unwrap(), "USD");
}

/// 折算基准跟随账本级设置：基准设为 USD 后，EUR 按 EUR→USD 汇率折算。
#[test]
fn convert_to_native_current_follows_base_currency_setting() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "EUR", "USD", 1.1);
    ledger_currencies::set_base_currency(&conn, "USD").unwrap();
    assert_eq!(
        convert_to_native_current(&conn, 10000, "EUR").unwrap(),
        11000
    );
}

/// 非默认币种按汇率折算到全局默认币种。
#[test]
fn convert_to_native_current_uses_rate_to_default_currency() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "USD").unwrap(),
        72000
    );
}

/// 折算基准是全局默认币种，与账户币种无关：
/// 即使存在 USD 账户，USD 金额仍折算到 CNY，而非 1:1 落库。
#[test]
fn convert_to_native_current_is_independent_of_account_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "USD").unwrap(),
        72000
    );
}

/// 只有反向汇率时取倒数折算。
#[test]
fn convert_to_native_current_uses_reverse_rate_when_only_reverse_exists() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "CNY", "EUR", 0.13);
    // 1 EUR = 1/0.13 CNY ≈ 7.6923
    assert_eq!(
        convert_to_native_current(&conn, 10000, "EUR").unwrap(),
        76923
    );
}

/// 正反向汇率均无 → 报错（不允许静默 1:1 混币种相加）。
#[test]
fn convert_to_native_current_errors_without_rate() {
    let conn = test_support::open();
    assert!(convert_to_native_current(&conn, 10000, "JPY").is_err());
}

/// 非正汇率（正查或反查）应报错，不得静默产出 0/负本位币金额。
#[test]
fn convert_to_native_current_rejects_non_positive_rate() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 0.0);
    assert!(convert_to_native_current(&conn, 10000, "USD").is_err());

    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "CNY", "EUR", -0.13);
    assert!(convert_to_native_current(&conn, 10000, "EUR").is_err());
}

/// 读路径回归（#1547）：当期入口只读当期表，不因汇率历史有点而改源——
/// 无当期行即报错（即使历史序列有点）；有当期行时用当期值而非历史值。
#[test]
fn convert_to_native_current_ignores_fx_rate_history() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-r", "USD", "CNY", "2026-01-05", 7.5);
    assert!(convert_to_native_current(&conn, 10000, "USD").is_err());

    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "USD").unwrap(),
        72000
    );
}

// ---------------------------------------------------------------------------
// convert_to_native_on_trade_date（按交易日折算，#1547：写路径取数汇率历史）
// ---------------------------------------------------------------------------

/// 与本位币同币种 → 原样返回、无折算留痕（金额与两个溯源列均为空值语义），
/// 与当期入口一致（#1541 验收：新入口对这条共同不变量可被单测直接调用；
/// 空值语义为 #1548 验收 3 的接缝级断言）。
#[test]
fn convert_to_native_on_trade_date_same_currency_is_identity_matches_current() {
    let conn = test_support::open();
    let conv = convert_to_native_on_trade_date(
        &conn,
        12345,
        &default_currency_code(&conn).unwrap(),
        "2026-01-07",
        None,
    )
    .unwrap();
    assert_eq!(conv.native_cents, 12345);
    assert_eq!(conv.fx_rate_used, None, "同币种不折算：无汇率留痕");
    assert_eq!(conv.fx_rate_source, None, "同币种不折算：无来源留痕");
}

/// 历史某周的交易按该周汇率折算（断言到分）：周一为周键，周内任一日期同值；
/// 相邻周各用自己的点，互不串周（issue #1547 验收 1）。
#[test]
fn convert_to_native_on_trade_date_uses_week_rate_of_trade_date() {
    let conn = test_support::open();
    // 2026-01-05 为周一：种子落 2026-01-05 当周（7.5）与下一周（8.0）各一点。
    test_support::seed_fx_rate_history(&conn, "fxh-w1", "USD", "CNY", "2026-01-05", 7.5);
    test_support::seed_fx_rate_history(&conn, "fxh-w2", "USD", "CNY", "2026-01-12", 8.0);
    // 周三 2026-01-07 与周日 2026-01-11 都命中同一周键，按该周汇率折算到分。
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        75000
    );
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 12345, "USD", "2026-01-11", None)
            .unwrap()
            .native_cents,
        92588
    );
    // 下周的交易用下周的点（12345 × 8.0）。
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 12345, "USD", "2026-01-13", None)
            .unwrap()
            .native_cents,
        98760
    );
    // 港币交易按交易周折算（#1547 验收字面）：52508160 分 × 0.9 = 47257344 分。
    test_support::seed_fx_rate_history(&conn, "fxh-hkd", "HKD", "CNY", "2026-01-05", 0.9);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 52_508_160, "HKD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        47_257_344
    );
}

/// 折算来源留痕（#1548 验收 2）：序列命中 → 汇率值存**使用值**（正查存序列点
/// 本身、反向兑底存倒数），来源标为 `series`——仅凭留痕即可复算行内本位币金额。
#[test]
fn convert_to_native_on_trade_date_traces_series_rate_and_source() {
    let conn = test_support::open();
    // 正查：留痕 = 序列点本身（10000 × 7.5 = 75000 分，四舍五入到分）。
    test_support::seed_fx_rate_history(&conn, "fxh-fwd", "USD", "CNY", "2026-01-05", 7.5);
    let conv = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap();
    assert_eq!(conv.fx_rate_used, Some(7.5));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(
        conv.native_cents, 75000,
        "native = amount × 留痕汇率（到分）"
    );

    // 反向兑底：序列只有 CNY→USD 点，使用值为倒数（1 / 0.125 = 8）。
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-rev", "CNY", "USD", "2026-01-05", 0.125);
    let conv = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap();
    assert_eq!(conv.fx_rate_used, Some(8.0), "反向命中留痕存使用值（倒数）");
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(conv.native_cents, 80000);
}

/// 折算来源闭集（#1548）：DB/wire 字符串边界严格互转，`series` 与 `explicit`
///（#1549 接入的调用方显式给定）两值同源同序；未知值报码化错误不静默。
#[test]
fn fx_rate_source_closed_set_roundtrip_and_rejects_unknown() {
    assert_eq!(
        FxRateSource::ALL,
        [FxRateSource::Series, FxRateSource::Explicit]
    );
    for source in FxRateSource::ALL {
        assert_eq!(FxRateSource::parse(source.as_str()).unwrap(), source);
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, format!("\"{}\"", source.as_str()));
        let back: FxRateSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }
    let err = FxRateSource::parse("guessed").unwrap_err();
    assert_eq!(err.code(), Some("fx.source-unknown"));
    assert_eq!(
        err.to_string(),
        "未知折算来源: guessed（合法值: series/explicit）",
        "message 逐字锁定（ADR-0050 只增不改）"
    );
}

/// 只有反向序列点时取倒数折算（正反向兜底，与当期入口同规则）。
#[test]
fn convert_to_native_on_trade_date_uses_reverse_rate_when_only_reverse_exists() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-rev", "CNY", "USD", "2026-01-06", 0.125);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        80000
    );
}

/// 交易所属周整周无点 → 既有 `fx.rate-missing` 码化错误，且不滑到相邻周、
/// 不回落当期表：相邻周与当期表都有值，仍报错（issue #1547）。
#[test]
fn convert_to_native_on_trade_date_errors_when_whole_week_missing() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-next", "USD", "CNY", "2026-01-12", 8.0);
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    let err = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap_err();
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert_eq!(params, vec!["USD".to_string(), "CNY".to_string()]);
        }
        other => panic!("应为中心化缺失码化错误，实际: {other}"),
    }
}

/// 整周无点的文案区分（issue #1547 验收 2）：当周（含未到周）缺点 →
/// 「该周尚未发布，待汇率同步后重试即可」；历史周缺点 → 「该周历史空缺」。
#[test]
fn convert_to_native_on_trade_date_missing_week_copy_distinguishes_current_from_gap() {
    // 当周：周一取自真实时钟（本周一无点 → 尚未发布，重试即可）。
    let conn = test_support::open();
    let this_monday: String = conn
        .query_row(
            "SELECT date('now','localtime','-6 days','weekday 1')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let err = convert_to_native_on_trade_date(&conn, 10000, "USD", &this_monday, None).unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert!(message.contains("尚未发布"), "实际: {message}");
            assert!(message.contains("重试"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }

    // 历史周：固定过去日期缺点 → 历史空缺（重试无济于事）。
    let conn = test_support::open();
    let err = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert!(message.contains("历史空缺"), "实际: {message}");
            assert!(!message.contains("尚未发布"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }
}

/// 序列点非正（正查或反查）报错，不得静默产出 0/负本位币金额。
#[test]
fn convert_to_native_on_trade_date_rejects_non_positive_rate() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-zero", "USD", "CNY", "2026-01-05", 0.0);
    assert!(convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).is_err());

    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-neg", "CNY", "USD", "2026-01-05", -0.13);
    assert!(convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).is_err());
}

// convert_to_native_on_edit（编辑沿用，#1550：币种/金额/日期未变沿用行内留痕）
// ---------------------------------------------------------------------------

/// 沿用基线构造器（本节测试专用）：旧行三元组 + 行内留痕。
fn reuse_baseline(
    amount_cents: i64,
    currency_code: &str,
    date: &str,
    conversion: NativeConversion,
) -> FxEditBaseline {
    FxEditBaseline {
        amount_cents,
        currency_code: currency_code.into(),
        date: date.into(),
        conversion,
    }
}

/// 三元组（金额/币种/日期）未变 → 原样返回行内留痕，不再查序列：序列整表为空
/// 仍成功，且留痕逐位不变（#1550 验收 1——只改备注/分类/商户的场景，序列已
/// 清空也改得动，导入的历史行不因数据源查不到当年值而不可编辑）。
#[test]
fn convert_to_native_on_edit_reuses_inline_trace_when_triple_unchanged() {
    let conn = test_support::open();
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 900, "本位币金额逐位不变");
    assert_eq!(conv.fx_rate_used, Some(0.9), "汇率值逐位不变");
    assert_eq!(
        conv.fx_rate_source,
        Some(FxRateSource::Series),
        "来源逐位不变"
    );
}

/// 三元组未变但行内留痕为空（存量行写入早于留痕功能，或同币种未折算）→
/// 空值原样保留：只改备注不得把存量行的本位币金额与留痕改写（#1548 空值语义
/// 在编辑路径的延续）。
#[test]
fn convert_to_native_on_edit_reuses_null_trace_when_triple_unchanged() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-legacy", "HKD", "CNY", "2026-01-05", 0.9);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 1000,
            fx_rate_used: None,
            fx_rate_source: None,
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 1000, "存量行金额不改写");
    assert_eq!(conv.fx_rate_used, None, "存量行空留痕不改写");
    assert_eq!(conv.fx_rate_source, None, "存量行空来源不改写");
}

/// 金额变了 → 按新金额重查序列，不沿用旧留痕（#1550 验收 2）。
#[test]
fn convert_to_native_on_edit_requeries_when_amount_changed() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-amt", "HKD", "CNY", "2026-01-05", 0.8);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 2000, "HKD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 1600, "按新金额与新序列值重算");
    assert_eq!(conv.fx_rate_used, Some(0.8), "留痕更新为本笔使用的序列值");
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

/// 日期变了 → 按新日期所属周重查，用新周点（不沿用旧周留痕）。
#[test]
fn convert_to_native_on_edit_requeries_when_date_changed() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-w1", "HKD", "CNY", "2026-01-05", 0.9);
    test_support::seed_fx_rate_history(&conn, "fxh-w2", "HKD", "CNY", "2026-01-12", 0.95);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-14", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 950, "按新日期所属周重算");
    assert_eq!(conv.fx_rate_used, Some(0.95));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

/// 币种变了 → 按新币种重查（旧币种留痕不沿用）。
#[test]
fn convert_to_native_on_edit_requeries_when_currency_changed() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-usd", "USD", "CNY", "2026-01-05", 7.1);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "USD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 7100, "按新币种序列值重算");
    assert_eq!(conv.fx_rate_used, Some(7.1));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

/// 三元组任一变了且新周查不到 → 既有 `fx.rate-missing` 码化错误失败，
/// 不静默沿用旧值（#1550 验收 3）；当期表有点也不回落（与按交易日入口同规）。
#[test]
fn convert_to_native_on_edit_fails_coded_when_changed_and_week_missing() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "HKD", "CNY", 0.91);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let err = convert_to_native_on_edit(&conn, 2000, "HKD", "2026-01-07", None, Some(&baseline))
        .unwrap_err();
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert_eq!(params, vec!["HKD".to_string(), "CNY".to_string()]);
        }
        other => panic!("应为既有码化缺失错误，实际: {other}"),
    }
}

/// 基线缺席（创建路径）与按交易日入口完全一致：同币种 1:1、无留痕；外币按
/// 序列折算并留痕（写入路径零变化）。
#[test]
fn convert_to_native_on_edit_without_baseline_matches_trade_date_entry() {
    let conn = test_support::open();
    let conv = convert_to_native_on_edit(&conn, 12345, "CNY", "2026-01-07", None, None).unwrap();
    assert_eq!(conv.native_cents, 12345);
    assert_eq!(conv.fx_rate_used, None);
    assert_eq!(conv.fx_rate_source, None);

    test_support::seed_fx_rate_history(&conn, "fxh-new", "USD", "CNY", "2026-01-05", 7.5);
    let conv = convert_to_native_on_edit(&conn, 10000, "USD", "2026-01-07", None, None).unwrap();
    assert_eq!(conv.native_cents, 75000);
    assert_eq!(conv.fx_rate_used, Some(7.5));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

/// 写路径入口只认汇率历史，不回落当期表（#1547）：当期表有点、序列无点 →
/// 报错而非拿当期值顶上；序列有点时用序列值，而非当期表值。
#[test]
fn convert_to_native_on_trade_date_does_not_fall_back_to_current_table() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert!(convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).is_err());

    test_support::seed_fx_rate_history(&conn, "fxh-cur", "USD", "CNY", "2026-01-05", 7.5);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        75000
    );
}

// ---------------------------------------------------------------------------
// 逐笔显式汇率（#1549：写入契约可选入参，显式 > 序列命中 > 报错）
// ---------------------------------------------------------------------------

/// 带显式汇率的一笔跳过序列查询、按给定汇率折算，留痕标为 `explicit`
/// （#1549 验收 1）：交易周有序列点仍不取序列值；序列整周无点（数据源覆盖
/// 不到的日期）也照常折算——显式汇率正是为这个缺口而生。
#[test]
fn convert_to_native_on_trade_date_explicit_rate_skips_series_and_traces_explicit() {
    // 交易周有序列点：显式值优先（explicit 0.88 ≠ series 0.9）。
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-hit", "HKD", "CNY", "2026-01-05", 0.9);
    let conv = convert_to_native_on_trade_date(&conn, 52_508_160, "HKD", "2026-01-07", Some(0.88))
        .unwrap();
    assert_eq!(
        conv.native_cents, 46_207_181,
        "native = amount × 显式汇率（到分）"
    );
    assert_eq!(conv.fx_rate_used, Some(0.88), "留痕 = 显式给定值本身");
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Explicit));

    // 序列整周无点（缺口日期）：显式汇率照常折算，不报 fx.rate-missing。
    let conn = test_support::open();
    let conv =
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", Some(7.19)).unwrap();
    assert_eq!(conv.native_cents, 71900);
    assert_eq!(conv.fx_rate_used, Some(7.19));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Explicit));
}

/// 显式汇率非法 → 码化错误、不落库（#1549 验收 3）：非正（0 / 负值）与非有限
/// （NaN / 无穷大）均报 `fx.explicit-rate-non-positive`。
#[test]
fn convert_to_native_on_trade_date_rejects_non_positive_explicit_rate() {
    for rate in [0.0, -0.85, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let conn = test_support::open();
        let err = convert_to_native_on_trade_date(&conn, 10000, "HKD", "2026-01-07", Some(rate))
            .unwrap_err();
        match err {
            AppError::Coded { code, params, .. } => {
                assert_eq!(code, "fx.explicit-rate-non-positive");
                assert_eq!(params, vec![rate.to_string()]);
            }
            other => panic!("应为码化错误，实际: {other}"),
        }
    }
}

/// 与本位币同币种的行无折算方向，携带显式汇率即「方向不符」码化错误
/// （#1549 验收 3 的方向条件）：该笔本就 1:1 原样返回，显式汇率无处安放，
/// fail fast 不静默吞掉。
#[test]
fn convert_to_native_on_trade_date_rejects_explicit_rate_on_same_currency_row() {
    let conn = test_support::open();
    let native = default_currency_code(&conn).unwrap();
    let err = convert_to_native_on_trade_date(&conn, 10000, &native, "2026-01-07", Some(1.0))
        .unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "fx.explicit-rate-direction-mismatch");
            assert!(message.contains("方向不符"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }
    // 同币种不带显式汇率的行为不变（#1549 验收 2：既有行为逐位一致）。
    let conv = convert_to_native_on_trade_date(&conn, 12345, &native, "2026-01-07", None).unwrap();
    assert_eq!(conv.native_cents, 12345);
    assert_eq!(conv.fx_rate_used, None);
    assert_eq!(conv.fx_rate_source, None);
}

/// 编辑时调用方逐笔显式给定汇率（#1549 × #1550）：显式 > 沿用 > 序列——三元组
/// 未变也不沿用行内留痕，按给定汇率折算、来源标为 `Explicit`（沿用吞掉显式
/// 等于无视调用方指令）；显式非法照走按交易日入口的既有码化校验。
#[test]
fn convert_to_native_on_edit_explicit_rate_beats_baseline_reuse() {
    let conn = test_support::open();
    // 序列整表为空：显式在场不查序列；基线三元组与请求完全一致。
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv = convert_to_native_on_edit(
        &conn,
        1000,
        "HKD",
        "2026-01-07",
        Some(0.88),
        Some(&baseline),
    )
    .unwrap();
    assert_eq!(conv.native_cents, 880, "按显式给定值折算");
    assert_eq!(conv.fx_rate_used, Some(0.88));
    assert_eq!(
        conv.fx_rate_source,
        Some(FxRateSource::Explicit),
        "来源改标显式"
    );

    // 显式值非法（非正）→ 既有码化错误，不落旧值。
    let err =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-07", Some(0.0), Some(&baseline))
            .unwrap_err();
    assert_eq!(err.code(), Some("fx.explicit-rate-non-positive"));
}
