//! 投资交易写入与买卖明细测试（命名对齐源码 trade 模块）：buy 建 lot、Amount
//! 接缝折算、非投资账户拒绝、sell FIFO 多 lot 匹配、超卖拒绝、盈亏扣费、
//! get_transaction_trade 明细与缺失拒绝（issue #257 纯移动归组）。

use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::create_transaction_internal;
use rusqlite::{Connection, params};

use super::super::*;
use super::common::*;
use crate::test_support::{open, seed_account, seed_exchange_rate, seed_instrument};

/// 价格刻度不变式（ADR-0038）：四类价格列（成交单价/每份成本/现价/价格历史）
/// 以万分之一元（0.0001 元）存储，金额列仍为整数分——金额分 = 数量 × 单价 ÷ 100。
/// 用 4 位小数基金净值（无法用「分」无损表示的形态）钉住刻度：
/// 申购 3 份 @ 1.2345 元 + 1 元费 → 行金额 4.70 元（470 分），每份成本摊薄为
/// 1.5678 元（15678）；赎回 3 份 @ 1.30 元 → 收入 3.90 元，已实现盈亏 −0.80 元。
#[test]
fn price_scale_invariant_unit_price_is_ten_thousandths_of_yuan() {
    let conn = open();
    seed_account(&conn, "acc-fund", "基金户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fund-abc",
        "000123",
        "某公募基金",
        "CNY",
        "unknown",
    );

    // 申购：数量 3、单价 1.2345 元（存 12345 万分之一元）、手续费 1 元（100 分）。
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-fund", "inst-fund-abc", 3.0, 12345, 100),
    )
    .unwrap()
    .id;

    let (amount_cents, price_cents): (i64, i64) = conn
        .query_row(
            "SELECT t.amount_cents, st.price_cents FROM transactions t \
             JOIN security_transactions st ON st.transaction_id = t.id WHERE t.id=?1",
            params![buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        price_cents, 12345,
        "成交单价无损保留 4 位小数净值（1.2345 元）"
    );
    assert_eq!(
        amount_cents, 470,
        "行金额 = 3 × 1.2345 + 1 = 4.70 元 = 470 分"
    );

    let cost_per_unit: i64 = conn
        .query_row(
            "SELECT cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cost_per_unit, 15678,
        "每份成本 = (3×12345 + 100×100)/3 = 15678 万分之一元"
    );

    // 赎回：全部 3 份 @ 1.30 元（存 13000），无费。已实现盈亏为整数分。
    let sell_id = create_transaction_internal(
        &conn,
        make_sell_input("acc-fund", "inst-fund-abc", 3.0, 13000, 0),
    )
    .unwrap()
    .id;
    let (sell_amount, realized_pnl): (i64, i64) = conn
        .query_row(
            "SELECT t.amount_cents, sls.realized_pnl_cents FROM transactions t \
             JOIN security_lot_sales sls ON sls.sell_transaction_id = t.id WHERE t.id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sell_amount, 390, "卖出收入 = 3 × 1.30 = 3.90 元 = 390 分");
    assert_eq!(realized_pnl, 390 - 470, "已实现盈亏 = 收入 − 成本（均分）");
}

/// 持仓视图刻度不变式：市值（分）= 数量 × 现价（万分之一元）÷ 100，
/// 成本（分）= 数量 × 每份成本（万分之一元）÷ 100（v_holdings 表达式与 V002 同源）。
#[test]
fn price_scale_invariant_v_holdings_market_value() {
    let conn = open();
    seed_account(&conn, "acc-scale", "刻度户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-scale",
        "000456",
        "净值保真基金",
        "CNY",
        "unknown",
    );

    // 买入 100 份 @ 0.1234 元（存 1234 万分之一元），无费。
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-scale", "inst-scale", 100.0, 1234, 0),
    )
    .unwrap()
    .id;

    let now = crate::db::now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,5678,'CNY',?3,NULL,?4,?5,?6,?7)",
        params![crate::db::new_uuid(), "inst-scale", now, now, now, 1, "test"],
    )
    .unwrap();

    let (cost_basis, market_value): (i64, i64) = conn
        .query_row(
            "SELECT cost_basis_cents, market_value_cents FROM v_holdings WHERE instrument_id=?1",
            params!["inst-scale"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        cost_basis, 1234,
        "成本 = 100 × 1234 ÷ 100 = 1234 分 = 12.34 元"
    );
    assert_eq!(
        market_value, 5678,
        "市值 = 100 × 5678 ÷ 100 = 5678 分 = 56.78 元"
    );
    let _ = buy_id;
}

#[test]
fn buy_transaction_creates_lot() {
    let conn = open();
    seed_account(&conn, "acc-test-buy", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-test-nvda", "NVDA", "NVIDIA", "USD", "unknown");

    let input = make_buy_input("acc-test-buy", "inst-test-nvda", 10.0, 1_000_000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap().id;

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
    assert_eq!(
        price_cents, 1_000_000,
        "成交单价存万分之一元（100 元 = 1_000_000）"
    );
    assert_eq!(fee_cents, 500);

    let (remaining_quantity, cost_per_unit): (f64, i64) = conn
        .query_row(
            "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((remaining_quantity - 10.0).abs() < 0.0001);
    assert_eq!(
        cost_per_unit, 1_005_000,
        "每份成本存万分之一元（含费用摊薄）"
    );

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
    let conn = open();
    seed_account(&conn, "acc-test-conv", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    seed_instrument(&conn, "inst-test-conv", "NVDA", "NVIDIA", "USD", "unknown");

    let input = make_buy_input("acc-test-conv", "inst-test-conv", 10.0, 1_000_000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap().id;

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
    use crate::transaction::update_transaction_internal;
    let conn = open();
    seed_account(&conn, "acc-test-conv-upd", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    seed_instrument(
        &conn,
        "inst-test-conv-upd",
        "NVDA",
        "NVIDIA",
        "USD",
        "unknown",
    );

    let txn_id = create_transaction_internal(
        &conn,
        make_buy_input(
            "acc-test-conv-upd",
            "inst-test-conv-upd",
            10.0,
            1_000_000,
            500,
        ),
    )
    .unwrap()
    .id;

    let mut edited = make_buy_input("acc-test-conv-upd", "inst-test-conv-upd", 5.0, 1_200_000, 0);
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

/// sell 本位币金额同样经 Amount 接缝折算（issue #771 自 API 集成层移交域权威）：
/// 非 1:1 汇率下卖出行落库的 `amount_native_cents` 为折算值而非原始金额。
#[test]
fn sell_native_cents_converted_via_amount_seam() {
    let conn = open();
    seed_account(&conn, "acc-test-sell-conv", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    seed_instrument(
        &conn,
        "inst-test-sell-conv",
        "TSLA",
        "Tesla",
        "USD",
        "unknown",
    );

    // 前置买入建仓，卖出按 FIFO 消费持仓。
    create_transaction_internal(
        &conn,
        make_buy_input(
            "acc-test-sell-conv",
            "inst-test-sell-conv",
            10.0,
            1_000_000,
            0,
        ),
    )
    .unwrap();
    let sell_id = create_transaction_internal(
        &conn,
        make_sell_input(
            "acc-test-sell-conv",
            "inst-test-sell-conv",
            4.0,
            1_100_000,
            0,
        ),
    )
    .unwrap()
    .id;

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 44000, "卖出入账 = 数量×单价−手续费");
    assert_eq!(
        amount_native_cents, 316800,
        "本位币金额应经 convert_to_native 折算（44000 × 7.2）"
    );
}

#[test]
fn buy_transaction_requires_investment_account() {
    let conn = open();
    seed_account(&conn, "acc-test-cash", "现金", "cash", "CNY", 0);
    seed_instrument(&conn, "inst-test-cny", "600519", "茅台", "CNY", "unknown");

    let input = make_buy_input("acc-test-cash", "inst-test-cny", 1.0, 10000, 0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    assert!(
        err.to_string().contains("投资账户"),
        "非投资账户买入应报错，got: {err}"
    );
}

/// 拒绝理由表行（#768）：行 =（行名，期望文案，动作闭包，可选后置闭包）。
/// 行名以写入口动作开头，兼作失败输出中的动作标识；期望文案为拒绝消息必须
/// 逐条 `contains` 的子串清单（拒绝文案与标的 id，逐条对应原三胞胎断言面）。
struct RejectionRow {
    name: &'static str,
    expected: &'static [&'static str],
    /// 动作闭包：自备前置（种子/建仓）并触发目标写入口；返回 Ok 即意外成功。
    action: fn(&Connection) -> Result<(), AppError>,
    /// 可选后置闭包：拒绝生效后的残留/原值断言。
    post: Option<fn(&Connection)>,
}

/// 标的不存在的拒绝理由表（#768）：原三胞胎（buy 建仓 / sell 建仓 / buy 修改）
/// 拒绝断言体逐字同构，收敛为行数据 + 单一断言体，新增拒绝理由 = 加一行数据。
/// 断言面原样保留（issue #295）：引用不存在标的在 prepare 校验段拦截为码化
/// [`AppError::Coded`]（HTTP 侧 400），不再等到 apply 落 `security_transactions`
/// 触发外键违规的「数据库错误」500；sell 侧先于可卖数量校验（否则误报
/// 「可卖出数量不足，当前持有 0」，语义不明）；文案与标的 id 的 contains 断言
/// 逐条对应原三处。第三行「保留原值」后置断言作可选后置列，不丢断言面。
/// 失败报行：输出含行名（动作标识）、期望文案与实际消息。跨层重复（BDD 镜像
/// 场景、api_server 400 断言）不进本表，归 #730 测试层级重划。
#[test]
fn missing_instrument_rejection_reason_table() {
    use crate::transaction::update_transaction_internal;

    let rows: &[RejectionRow] = &[
        // buy_with_missing_instrument_rejected_as_invalid_in_prepare
        RejectionRow {
            name: "buy_create 引用不存在标的",
            expected: &["买入标的不存在", "inst-not-exist"],
            action: |conn| {
                seed_account(conn, "acc-test-missing", "美股", "investment", "USD", 0);
                seed_exchange_rate(conn, "USD", "CNY", 1.0);
                let input = make_buy_input("acc-test-missing", "inst-not-exist", 10.0, 10000, 0);
                create_transaction_internal(conn, input).map(|_| ())
            },
            post: Some(|conn| {
                // prepare 拦截：交易行与持仓/明细均无落库残留。
                let txns: i64 = conn
                    .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(txns, 0, "被拒的买入不应落交易行");
                let lots: i64 = conn
                    .query_row("SELECT COUNT(*) FROM security_lots", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(lots, 0, "被拒的买入不应有持仓批次");
            }),
        },
        // sell_with_missing_instrument_rejected_as_invalid_in_prepare
        RejectionRow {
            name: "sell_create 引用不存在标的",
            expected: &["卖出标的不存在", "inst-not-exist"],
            action: |conn| {
                seed_account(conn, "acc-test-sell-miss", "美股", "investment", "USD", 0);
                seed_exchange_rate(conn, "USD", "CNY", 1.0);
                let input = make_sell_input("acc-test-sell-miss", "inst-not-exist", 5.0, 12000, 0);
                create_transaction_internal(conn, input).map(|_| ())
            },
            post: None,
        },
        // update_buy_to_missing_instrument_rejected_and_keeps_original
        RejectionRow {
            name: "update_buy 改引不存在标的",
            expected: &["买入标的不存在"],
            action: |conn| {
                seed_account(conn, "acc-test-upd-miss", "美股", "investment", "USD", 0);
                seed_exchange_rate(conn, "USD", "CNY", 1.0);
                seed_instrument(
                    conn,
                    "inst-test-upd-miss",
                    "NVDA",
                    "NVIDIA",
                    "USD",
                    "unknown",
                );
                let txn_id = create_transaction_internal(
                    conn,
                    make_buy_input(
                        "acc-test-upd-miss",
                        "inst-test-upd-miss",
                        10.0,
                        1_000_000,
                        0,
                    ),
                )
                .unwrap()
                .id;
                // 修改（全字段替换）路径同样在 prepare 拦截：改引不存在标的 → Invalid，
                // 入口自持事务整体回滚（revert→plan 中途失败不留中间态）。
                let edited =
                    make_buy_input("acc-test-upd-miss", "inst-not-exist", 5.0, 1_200_000, 0);
                update_transaction_internal(conn, &txn_id, edited).map(|_| ())
            },
            post: Some(|conn| {
                // 原交易行与持仓批次保持原样（revert→plan 中途失败整体回滚）：
                // 库中唯一交易行即原买入、唯一批次即其持仓，断言其金额与剩余
                // 数量未被改动（与原测试同面：只锁原行值，不断总行数）。
                let amount_cents: i64 = conn
                    .query_row("SELECT amount_cents FROM transactions", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(amount_cents, 100000, "原交易金额不应被修改");
                let remaining: f64 = conn
                    .query_row("SELECT remaining_quantity FROM security_lots", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                assert!((remaining - 10.0).abs() < 1e-9, "原持仓批次不应被清理");
            }),
        },
    ];
    for row in rows {
        let conn = open();
        let err = match (row.action)(&conn) {
            Err(err) => err,
            Ok(()) => panic!("拒绝理由表行「{}」失败：动作意外成功，应被拒绝", row.name),
        };
        match err {
            AppError::Coded { message, .. } => {
                for expected in row.expected {
                    assert!(
                        message.contains(expected),
                        "拒绝理由表行「{}」失败：期望文案「{expected}」未出现在拒绝消息中，got: {message}",
                        row.name
                    );
                }
            }
            other => panic!(
                "拒绝理由表行「{}」失败：应返回 Coded（400），got: {other:?}",
                row.name
            ),
        }
        if let Some(post) = row.post {
            post(&conn);
        }
    }
}

#[test]
fn sell_transaction_matches_multiple_lots_fifo() {
    let conn = open();
    seed_account(&conn, "acc-test-sell", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-test-sell", "TSLA", "Tesla", "USD", "unknown");

    let lot1_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 10.0, 1_000_000, 0),
    )
    .unwrap()
    .id;
    let lot2_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 5.0, 1_200_000, 0),
    )
    .unwrap()
    .id;

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
        make_sell_input("acc-test-sell", "inst-test-sell", 12.0, 1_500_000, 600),
    )
    .unwrap()
    .id;

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
    assert_eq!(
        rows[0].1, 1_000_000,
        "卖出时批次单位成本快照同为万分之一元刻度"
    );
    assert_eq!(rows[0].2, 49500);
    assert_eq!(rows[0].3, "USD");
    assert!((rows[1].0 - 2.0).abs() < 0.0001);
    assert_eq!(rows[1].1, 1_200_000);
    assert_eq!(rows[1].2, 5900);
    assert_eq!(rows[1].3, "USD");
}

#[test]
fn sell_transaction_rejects_oversell() {
    let conn = open();
    seed_account(&conn, "acc-test-oversell", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(
        &conn,
        "inst-test-oversell",
        "MSFT",
        "Microsoft",
        "USD",
        "unknown",
    );

    create_transaction_internal(
        &conn,
        make_buy_input("acc-test-oversell", "inst-test-oversell", 5.0, 1_000_000, 0),
    )
    .unwrap();

    let sell = make_sell_input("acc-test-oversell", "inst-test-oversell", 6.0, 12000, 0);
    assert!(create_transaction_internal(&conn, sell).is_err());
}

#[test]
fn sell_transaction_pnl_deducts_fee() {
    let conn = open();
    seed_account(&conn, "acc-test-pnl", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-test-pnl", "AAPL", "Apple", "USD", "unknown");

    let buy_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-pnl", "inst-test-pnl", 10.0, 1_000_000, 0),
    )
    .unwrap()
    .id;
    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-pnl", "inst-test-pnl", 5.0, 1_200_000, 200),
    )
    .unwrap()
    .id;

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
    assert_eq!(cost, 1_000_000);
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
    let conn = open();
    seed_account(&conn, "acc-inv", "证券户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-t", "600519", "贵州茅台", "USD", "unknown");
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    let id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-t", 100.0, 150_000, 500),
    )
    .unwrap()
    .id;

    let trade = trade::get_transaction_trade(&conn, &id).unwrap();
    assert_eq!(trade.instrument_id, "inst-t");
    assert_eq!(trade.symbol, "600519");
    assert_eq!(trade.instrument_name.as_deref(), Some("贵州茅台"));
    assert_eq!(
        trade.instrument_type, "stock",
        "明细带出标的类型（issue #302 表单形态切换）"
    );
    assert!((trade.quantity - 100.0).abs() < 1e-9);
    assert_eq!(trade.price_cents, 150_000, "明细单价万分之一元刻度");
    assert_eq!(trade.fee_cents, Some(500));
}

#[test]
fn get_transaction_trade_rejects_missing_or_non_trade_transaction() {
    let conn = open();
    seed_account(&conn, "acc-cash", "现金", "cash", "CNY", 0);
    // 非买卖交易（expense）无买卖明细
    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-cash".into(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap()
    .id;
    let err = trade::get_transaction_trade(&conn, &expense_id).unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
    // 不存在的 id 同样 NotFound
    let err = trade::get_transaction_trade(&conn, "no-such-txn").unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
}

/// 可卖数量守卫容差（issue #1033）：f64 FIFO 扣减的累积位噪声不得误拒真实全清仓。
/// 复刻真实账本位模式：buy 11511.99 → convert 转出两腿 684.76、2791.12 → 批次剩余
/// 低于 8036.11 的位模式（issue 附 DB printf('%.17g') 证据：8036.109999999999）→
/// sell 8036.11 必须放行，且批次精确归零。
#[test]
fn sell_full_clearance_with_fifo_noise_is_allowed() {
    let conn = open();
    seed_account(&conn, "acc-noise", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(
        &conn,
        "inst-noise-src",
        "006793",
        "源标的",
        "USD",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-noise-dst",
        "519700",
        "目标标的",
        "USD",
        "unknown",
    );

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-noise", "inst-noise-src", 11511.99, 10_000, 0),
    )
    .unwrap()
    .id;
    // 多腿转换单按提交方拆成多条 convert 记录。腿序决定噪声方向（IEEE 双精度
    // 逐次扣减不结合）：11511.99 − 2791.12 − 684.76 = 8036.109999999999（issue
    // 位模式），反序则恰好干净——守卫必须对两种序同等容忍。
    create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-noise",
            "inst-noise-src",
            "inst-noise-dst",
            2791.12,
            2791.12,
            279_112,
            279_112,
            0,
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-noise",
            "inst-noise-src",
            "inst-noise-dst",
            684.76,
            684.76,
            68_476,
            68_476,
            0,
        ),
    )
    .unwrap();

    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        remaining < 8036.11,
        "复刻位模式：FIFO 扣减后剩余应低于 8036.11（真实账本 8036.109999999999），got {remaining}"
    );

    // 此前被裸比较误拒的全清仓卖出：容差内放行。
    create_transaction_internal(
        &conn,
        make_sell_input("acc-noise", "inst-noise-src", 8036.11, 10_000, 0),
    )
    .unwrap();

    let remaining_after: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_after, 0.0, "全清仓后批次精确归零");
}

/// 容差边界钉住 1e-6：真实超卖（差额超容差）仍以同码拒绝；容差内的位噪声差异放行。
#[test]
fn sell_guard_rejects_oversell_beyond_tolerance() {
    let conn = open();
    seed_account(&conn, "acc-eps", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-eps", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-eps", "inst-eps", 100.0, 10_000, 0),
    )
    .unwrap();

    // 差额 2e-6 > 容差 1e-6：真实超卖，拒绝且同码。
    let err = create_transaction_internal(
        &conn,
        make_sell_input("acc-eps", "inst-eps", 100.000002, 10_000, 0),
    )
    .unwrap_err();
    assert!(
        err.is_code("trade.insufficient-holding"),
        "超容差超卖应保持原码拒绝，got: {err:?}"
    );

    // 差额 5e-7 < 容差 1e-6：位噪声量级，放行。
    create_transaction_internal(
        &conn,
        make_sell_input("acc-eps", "inst-eps", 100.0000005, 10_000, 0),
    )
    .unwrap();
}

/// 尘埃批次（issue #1033 同根因的另一面）：批次剩余噪声偏高时，全清仓不得留下
/// 1e-12 量级的残余批次（否则 v_holdings/InvestedInstrument 永远认其为持仓）。
/// 噪声偏高位模式无公开写入口可确定性产生，按 ADR-0086 以裸 SQL 直置（模拟历史
/// f64 扣减残差），显式登记于此。
#[test]
fn sell_full_clearance_consumes_noise_over_lot_exactly() {
    let conn = open();
    seed_account(&conn, "acc-dust", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-dust", "MSFT", "Microsoft", "USD", "unknown");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-dust", "inst-dust", 100.0, 10_000, 0),
    )
    .unwrap()
    .id;
    conn.execute(
        "UPDATE security_lots SET remaining_quantity=100.000000000001 WHERE buy_transaction_id=?1",
        params![buy_id],
    )
    .unwrap();

    create_transaction_internal(
        &conn,
        make_sell_input("acc-dust", "inst-dust", 100.0, 10_000, 0),
    )
    .unwrap();

    let remaining_after: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_after, 0.0, "清仓后批次精确归零，不留尘埃残差");
    let active_lots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE instrument_id='inst-dust' AND remaining_quantity > 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_lots, 0, "无残余活跃批次");
}

/// 「可卖出数量不足」文案不带位噪声（issue #1033 报障文案）：数字按录入粒度合同
/// （issue #416，至多四位小数）去尾零展示，8036.109999999999 → 8036.11。
#[test]
fn insufficient_holding_error_message_hides_float_noise() {
    let conn = open();
    seed_account(&conn, "acc-msg", "美股", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-msg", "TSLA", "Tesla", "USD", "unknown");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-msg", "inst-msg", 100.0, 10_000, 0),
    )
    .unwrap()
    .id;
    conn.execute(
        "UPDATE security_lots SET remaining_quantity=99.99999999999999 WHERE buy_transaction_id=?1",
        params![buy_id],
    )
    .unwrap();

    let err = create_transaction_internal(
        &conn,
        make_sell_input("acc-msg", "inst-msg", 200.0, 10_000, 0),
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("当前持有 100，尝试卖出 200"),
        "文案应按四位小数去尾零展示，got: {message}"
    );
    assert!(
        !message.contains("99.9"),
        "文案不应出现位噪声原样数字，got: {message}"
    );
}
