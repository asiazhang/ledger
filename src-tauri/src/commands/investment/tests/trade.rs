//! 投资交易写入与买卖明细测试（命名对齐源码 trade 模块）：buy 建 lot、Amount
//! 接缝折算、非投资账户拒绝、sell FIFO 多 lot 匹配、超卖拒绝、盈亏扣费、
//! get_transaction_trade 明细与缺失拒绝（issue #257 纯移动归组）。

use crate::commands::transactions::create_transaction_internal;
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;
use rusqlite::params;

use super::super::*;
use super::common::*;

#[test]
fn buy_transaction_creates_lot() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-buy", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-nvda", "NVDA", "NVIDIA", "USD");

    let input = make_buy_input("acc-test-buy", "inst-test-nvda", 10.0, 10000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap();

    let (kind, amount_cents, currency_code, amount_native, category_id, refund_of_id): (
        String,
        i64,
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT kind, amount_cents, currency_code, amount_native_cents, category_id, \
             refund_of_transaction_id FROM transactions WHERE id=?1",
            params![txn_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(kind, "buy");
    assert_eq!(amount_cents, 100500);
    assert_eq!(currency_code, "USD");
    assert_eq!(amount_native, amount_cents, "买入本位币与原始币种应 1:1");
    assert_eq!(category_id, None);
    assert_eq!(refund_of_id, None);

    let (action, quantity, price_cents, fee_cents): (String, f64, i64, i64) = conn
        .query_row(
            "SELECT action, quantity, price_cents, fee_cents FROM security_transactions WHERE transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(action, "buy");
    assert!((quantity - 10.0).abs() < 0.0001);
    assert_eq!(price_cents, 10000);
    assert_eq!(fee_cents, 500);

    let (remaining_quantity, cost_per_unit): (f64, i64) = conn
        .query_row(
            "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((remaining_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_per_unit, 10050);

    let (holding_quantity, cost_basis): (f64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings WHERE id=?1",
            params!["acc-test-buy-inst-test-nvda-USD"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((holding_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_basis, 100500);
}

/// buy 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：非 1:1 汇率下
/// 落库的 `amount_native_cents` 为折算值而非原始金额（旧行为硬编码 1:1）。
#[test]
fn buy_native_cents_converted_via_amount_seam() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-conv", "美股", "investment", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    insert_instrument(&conn, "inst-test-conv", "NVDA", "NVIDIA", "USD");

    let input = make_buy_input("acc-test-conv", "inst-test-conv", 10.0, 10000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap();

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 100500, "原始币种金额 = 数量×单价+手续费");
    assert_eq!(
        amount_native_cents, 723600,
        "本位币金额应经 convert_to_native 折算（100500 × 7.2）"
    );
}

/// 修改 buy 交易（行为层 revert→plan→apply 的 UPDATE 侧）同样经折算：非 1:1 汇率下
/// `amount_native_cents` 保持折算值（INSERT/UPDATE 共用 prepare，防回归）。
#[test]
fn buy_update_native_cents_converted_via_amount_seam() {
    use crate::commands::transactions::update_transaction_internal;
    let conn = setup_db();
    insert_account(&conn, "acc-test-conv-upd", "美股", "investment", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    insert_instrument(&conn, "inst-test-conv-upd", "NVDA", "NVIDIA", "USD");

    let txn_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-conv-upd", "inst-test-conv-upd", 10.0, 10000, 500),
    )
    .unwrap();

    let mut edited = make_buy_input("acc-test-conv-upd", "inst-test-conv-upd", 5.0, 12000, 0);
    edited.date = "2026-02-01".into();
    update_transaction_internal(&conn, &txn_id, edited).unwrap();

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 60000, "修改后金额 = 5×12000");
    assert_eq!(
        amount_native_cents, 432000,
        "修改后本位币金额应经 convert_to_native 折算（60000 × 7.2）"
    );
}

#[test]
fn buy_transaction_requires_investment_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-cash", "现金", "cash", "CNY");
    insert_instrument(&conn, "inst-test-cny", "600519", "茅台", "CNY");

    let input = make_buy_input("acc-test-cash", "inst-test-cny", 1.0, 10000, 0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    assert!(
        err.to_string().contains("投资账户"),
        "非投资账户买入应报错，got: {err}"
    );
}

/// buy 引用不存在的标的：prepare 校验段拦截为 [`AppError::Invalid`]（HTTP 侧 400）
/// 中文错误，不再等到 apply 落 `security_transactions` 才触发外键违规的
/// 「数据库错误」500，AI 可读错误回自纠（issue #295）。错误携带标的 id。
#[test]
fn buy_with_missing_instrument_rejected_as_invalid_in_prepare() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-missing", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");

    let input = make_buy_input("acc-test-missing", "inst-not-exist", 10.0, 10000, 0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    match err {
        AppError::Invalid(msg) => {
            assert!(
                msg.contains("买入标的不存在"),
                "应报买入标的不存在，got: {msg}"
            );
            assert!(
                msg.contains("inst-not-exist"),
                "错误应携带标的 id 供回自纠，got: {msg}"
            );
        }
        other => panic!("应返回 Invalid（400），got: {other:?}"),
    }
    // prepare 拦截：交易行与持仓/明细均无落库残留。
    let txns: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txns, 0, "被拒的买入不应落交易行");
    let lots: i64 = conn
        .query_row("SELECT COUNT(*) FROM security_lots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(lots, 0, "被拒的买入不应有持仓批次");
}

/// sell 引用不存在的标的：同样在 prepare 拦截为 Invalid——且必须先于可卖数量
/// 校验（否则会误报「可卖出数量不足，当前持有 0」，语义不明，issue #295）。
#[test]
fn sell_with_missing_instrument_rejected_as_invalid_in_prepare() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-sell-miss", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");

    let input = make_sell_input("acc-test-sell-miss", "inst-not-exist", 5.0, 12000, 0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    match err {
        AppError::Invalid(msg) => {
            assert!(
                msg.contains("卖出标的不存在"),
                "应报卖出标的不存在，got: {msg}"
            );
            assert!(
                msg.contains("inst-not-exist"),
                "错误应携带标的 id 供回自纠，got: {msg}"
            );
        }
        other => panic!("应返回 Invalid（400），got: {other:?}"),
    }
}

/// 修改（全字段替换）路径同样生效：把已有买入改为引用不存在的标的 → Invalid，
/// 原交易行与持仓批次保持不变（入口自持事务整体回滚，issue #295）。
#[test]
fn update_buy_to_missing_instrument_rejected_and_keeps_original() {
    use crate::commands::transactions::update_transaction_internal;
    let conn = setup_db();
    insert_account(&conn, "acc-test-upd-miss", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-upd-miss", "NVDA", "NVIDIA", "USD");

    let txn_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-upd-miss", "inst-test-upd-miss", 10.0, 10000, 0),
    )
    .unwrap();

    let edited = make_buy_input("acc-test-upd-miss", "inst-not-exist", 5.0, 12000, 0);
    let err = update_transaction_internal(&conn, &txn_id, edited).unwrap_err();
    match err {
        AppError::Invalid(msg) => {
            assert!(
                msg.contains("买入标的不存在"),
                "应报买入标的不存在，got: {msg}"
            );
        }
        other => panic!("应返回 Invalid（400），got: {other:?}"),
    }

    // 原交易行与持仓批次保持原样（revert→plan 中途失败整体回滚）。
    let amount_cents: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount_cents, 100000, "原交易金额不应被修改");
    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!((remaining - 10.0).abs() < 1e-9, "原持仓批次不应被清理");
}

#[test]
fn sell_transaction_matches_multiple_lots_fifo() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-sell", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-sell", "TSLA", "Tesla", "USD");

    let lot1_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 10.0, 10000, 0),
    )
    .unwrap();
    let lot2_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 5.0, 12000, 0),
    )
    .unwrap();

    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-10T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot1_txn],
    )
    .unwrap();
    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-15T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot2_txn],
    )
    .unwrap();

    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-sell", "inst-test-sell", 12.0, 15000, 600),
    )
    .unwrap();

    let (kind, amount_cents): (TransactionKind, i64) = conn
        .query_row(
            "SELECT kind, amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Sell);
    assert_eq!(amount_cents, 179400);

    let rem1: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot1_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem1 - 0.0).abs() < 0.0001);
    let rem2: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot2_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem2 - 3.0).abs() < 0.0001);

    let rows: Vec<(f64, i64, i64, String)> = conn
        .prepare(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1 ORDER BY quantity DESC",
        )
        .unwrap()
        .query_map(params![sell_txn], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2);
    assert!((rows[0].0 - 10.0).abs() < 0.0001);
    assert_eq!(rows[0].1, 10000);
    assert_eq!(rows[0].2, 49500);
    assert_eq!(rows[0].3, "USD");
    assert!((rows[1].0 - 2.0).abs() < 0.0001);
    assert_eq!(rows[1].1, 12000);
    assert_eq!(rows[1].2, 5900);
    assert_eq!(rows[1].3, "USD");
}

#[test]
fn sell_transaction_rejects_oversell() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-oversell", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-oversell", "MSFT", "Microsoft", "USD");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-test-oversell", "inst-test-oversell", 5.0, 10000, 0),
    )
    .unwrap();

    let sell = make_sell_input("acc-test-oversell", "inst-test-oversell", 6.0, 12000, 0);
    assert!(create_transaction_internal(&conn, sell).is_err());
}

#[test]
fn sell_transaction_pnl_deducts_fee() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-pnl", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-test-pnl", "AAPL", "Apple", "USD");

    let buy_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-pnl", "inst-test-pnl", 10.0, 10000, 0),
    )
    .unwrap();
    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-pnl", "inst-test-pnl", 5.0, 12000, 200),
    )
    .unwrap();

    let rem: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem - 5.0).abs() < 0.0001);

    let (qty, cost, pnl, ccy): (f64, i64, i64, String) = conn
        .query_row(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((qty - 5.0).abs() < 0.0001);
    assert_eq!(cost, 10000);
    assert_eq!(pnl, 9800);
    assert_eq!(ccy, "USD");

    let amount_cents: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount_cents, 5 * 12000 - 200);
}

#[test]
fn get_transaction_trade_returns_buy_detail_with_instrument_display() {
    let conn = setup_db();
    insert_account(&conn, "acc-inv", "证券户", "investment", "USD");
    insert_instrument(&conn, "inst-t", "600519", "贵州茅台", "USD");
    insert_rate_1_1(&conn, "USD");
    let id =
        create_transaction_internal(&conn, make_buy_input("acc-inv", "inst-t", 100.0, 1500, 500))
            .unwrap();

    let trade = trade::get_transaction_trade(&conn, &id).unwrap();
    assert_eq!(trade.instrument_id, "inst-t");
    assert_eq!(trade.symbol, "600519");
    assert_eq!(trade.instrument_name.as_deref(), Some("贵州茅台"));
    assert!((trade.quantity - 100.0).abs() < 1e-9);
    assert_eq!(trade.price_cents, 1500);
    assert_eq!(trade.fee_cents, Some(500));
}

#[test]
fn get_transaction_trade_rejects_missing_or_non_trade_transaction() {
    let conn = setup_db();
    insert_account(&conn, "acc-cash", "现金", "cash", "CNY");
    // 非买卖交易（expense）无买卖明细
    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-cash".into(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();
    let err = trade::get_transaction_trade(&conn, &expense_id).unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
    // 不存在的 id 同样 NotFound
    let err = trade::get_transaction_trade(&conn, "no-such-txn").unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
}
