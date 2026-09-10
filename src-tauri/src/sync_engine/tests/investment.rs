//! 投资域与 AI 导入路径的全域 op 产出与重放收敛（issue #861）：
//! buy/sell 三件套（含基金金额权威形态）、买卖删除、可卖数量依赖倒挂的
//! 挂起重投递自愈、标的字典 / 汇率 / 手动报价 / 现价录入，以及 AI 批量导入
//! 的 op 产出与 IdempotencyKey 去重独立性。

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::investment::{
    InstrumentInput, InstrumentType, add_fund_by_code_with, create_exchange_rate,
    create_instrument, create_market_price, delete_instrument, record_manual_price,
};
use crate::test_support::{self, seed_investment_setup};
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::behavior;

fn buy_input(
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents: Some(price),
        fee_cents: Some(fee),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

fn sell_input(
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Sell,
        ..buy_input(account_id, instrument_id, quantity, price, fee)
    }
}

/// 持仓批次业务快照（自然键 = 账户 × 标的 × 买入交易；批次行 id 各端独立生成，
/// 不参与状态等值判定——与交易行审计列同一取舍）。
fn read_lot(conn: &rusqlite::Connection, buy_tx_id: &str) -> Option<(f64, f64, i64, String)> {
    conn.query_row(
        "SELECT initial_quantity, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots WHERE buy_transaction_id = ?1",
        [buy_tx_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .ok()
}

/// 卖出匹配业务快照（自然键 = 卖出交易 × 买入交易；匹配行 id / lot id 各端独立）。
fn read_lot_sale(
    conn: &rusqlite::Connection,
    sell_tx_id: &str,
    buy_tx_id: &str,
) -> Option<(f64, i64, i64)> {
    conn.query_row(
        "SELECT s.quantity, s.cost_per_unit_cents, s.realized_pnl_cents \
         FROM security_lot_sales s \
         JOIN security_lots l ON l.id = s.lot_id \
         WHERE s.sell_transaction_id = ?1 AND l.buy_transaction_id = ?2",
        [sell_tx_id, buy_tx_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

#[test]
fn buy_sell_replay_converge_holdings_and_pnl() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "inst-1");
    seed_investment_setup(&conn_b, "acc-inv", "inst-1");

    // A 端：买入 100 份 @15.00（fee 5 分）→ 卖出 40 份；每步 wire 到 B 端。
    let buy_id = behavior::create(&conn_a, buy_input("acc-inv", "inst-1", 100.0, 1500, 5))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    let (lot_a, lot_b) = (read_lot(&conn_a, &buy_id), read_lot(&conn_b, &buy_id));
    assert_eq!(
        lot_a,
        Some((100.0, 100.0, 1505, "USD".into())),
        "买入批次（每份成本含费用摊薄单次舍入）"
    );
    assert_eq!(lot_a, lot_b, "买入重放后批次一致");

    let sell_id = behavior::create(&conn_a, sell_input("acc-inv", "inst-1", 40.0, 1500, 5))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    let (lot_a, lot_b) = (read_lot(&conn_a, &buy_id), read_lot(&conn_b, &buy_id));
    assert_eq!(lot_a, Some((100.0, 60.0, 1505, "USD".into())));
    assert_eq!(lot_a, lot_b, "卖出重放后剩余数量一致");
    let (sale_a, sale_b) = (
        read_lot_sale(&conn_a, &sell_id, &buy_id),
        read_lot_sale(&conn_b, &sell_id, &buy_id),
    );
    assert_eq!(sale_a, Some((40.0, 1505, -7)), "匹配与已实现盈亏确定性");
    assert_eq!(sale_a, sale_b, "卖出匹配重放后一致");

    // 全量重投递：幂等跳过，无第二次效果。
    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
    assert_eq!(read_lot(&conn_b, &buy_id), lot_a);
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn fund_buy_replay_keeps_amount_authority() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "fund-1");
    seed_investment_setup(&conn_b, "acc-inv", "fund-1");
    // 基金类型（金额权威：确认单整分金额 + 确认份额，单价反算）。
    for conn in [&conn_a, &conn_b] {
        conn.execute(
            "UPDATE instruments SET instrument_type='fund' WHERE id='fund-1'",
            [],
        )
        .unwrap();
    }

    // 金额 1000.00 元（100000 分）买 9500.00 份，零费用：单价反算 1052.63…，
    // 每份成本 = 权威金额锚定单次舍入 1053（确认单抄写即记账，ADR-0038）。
    let mut input = buy_input("acc-inv", "fund-1", 9500.0, 0, 0);
    input.amount_cents = 100_000;
    input.price_cents = None;
    let buy_id = behavior::create(&conn_a, input).unwrap().id;
    wire_in(&conn_b, &wire_out(&conn_a));

    let (lot_a, lot_b) = (read_lot(&conn_a, &buy_id), read_lot(&conn_b, &buy_id));
    assert_eq!(
        lot_a,
        Some((9500.0, 9500.0, 1053, "USD".into())),
        "基金每份成本锚定权威金额"
    );
    assert_eq!(lot_a, lot_b, "基金申赎重放后批次一致");
}

#[test]
fn buy_delete_replay_cleans_lot() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "inst-1");
    seed_investment_setup(&conn_b, "acc-inv", "inst-1");

    let buy_id = behavior::create(&conn_a, buy_input("acc-inv", "inst-1", 100.0, 1500, 5))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    behavior::delete(&conn_a, &buy_id).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    let deleted = |conn: &rusqlite::Connection| {
        conn.query_row(
            "SELECT is_deleted FROM transactions WHERE id=?1",
            [&buy_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
    };
    assert_eq!(deleted(&conn_a), 1);
    assert_eq!(deleted(&conn_b), 1, "删除重放后同软删");
    assert_eq!(read_lot(&conn_a, &buy_id), None, "买入删除清理持仓批次");
    assert_eq!(read_lot(&conn_b, &buy_id), None, "删除重放同样清理批次");
    // 删除重放不产出本地 op（ADR-0091：重放不得追加本地 op）：两端 op 日志全等
    // ——否则重放端会再发布一条 Delete，对端重放命中已删行即挂起（issue #980）。
    assert_eq!(
        read_ops(&conn_a).unwrap(),
        read_ops(&conn_b).unwrap(),
        "删除重放不产 op，两端日志全等"
    );
}

#[test]
fn sell_before_buy_parks_then_redelivery_self_heals() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "inst-1");
    seed_investment_setup(&conn_b, "acc-inv", "inst-1");

    let buy_id = behavior::create(&conn_a, buy_input("acc-inv", "inst-1", 100.0, 1500, 5))
        .unwrap()
        .id;
    let sell_id = behavior::create(&conn_a, sell_input("acc-inv", "inst-1", 40.0, 1500, 5))
        .unwrap()
        .id;

    // 依赖倒挂：只投递卖出 op（买入 op 未达）——可卖数量守卫失败，挂起不落日志。
    let ops = read_ops(&conn_a).unwrap();
    let wire_of_kind = |kind: TransactionKind| {
        ops.iter()
            .filter(|op| match &op.command {
                crate::sync_engine::DomainCommand::Transaction(
                    crate::transaction::TransactionCommand::Create { row, .. },
                ) => row.kind == kind,
                _ => false,
            })
            .map(|op| serde_json::to_string(op).unwrap())
            .collect::<Vec<_>>()
    };
    let sell_wire = wire_of_kind(TransactionKind::Sell);
    let buy_wire = wire_of_kind(TransactionKind::Buy);
    let reports = wire_in(&conn_b, &sell_wire);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.insufficient-holding"),
        "可卖数量不足挂起并携带既有码：{:?}",
        reports[0]
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起不落日志");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    // 买入 op 补达（只投 buy，挂起的 sell 仍在队列）；重投递 sell：重放成功
    // 并出队（不堆积、不复活）。
    wire_in(&conn_b, &buy_wire);
    assert_eq!(
        parked_ops(&conn_b).unwrap().len(),
        1,
        "buy 补达不自动解决挂起"
    );
    let reports = wire_in(&conn_b, &sell_wire);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_eq!(read_lot(&conn_b, &buy_id), read_lot(&conn_a, &buy_id));
    assert_eq!(
        read_lot_sale(&conn_b, &sell_id, &buy_id),
        read_lot_sale(&conn_a, &sell_id, &buy_id)
    );
}

#[test]
fn buy_replay_with_missing_instrument_parks() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "inst-1");
    // B 端只有账户，标的字典尚未同步（外键依赖缺失场景）。
    test_support::seed_account(&conn_b, "acc-inv", "美股", "investment", "USD", 0);

    behavior::create(&conn_a, buy_input("acc-inv", "inst-1", 100.0, 1500, 5)).unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.buy-instrument-not-found"),
        "标的不存在挂起待裁决：{:?}",
        reports[0]
    );
}

#[test]
fn instrument_dictionary_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    // 自建标的建档 → op；重放后字典行一致（幂等复用的改名 → Update op）。
    let inst_id = create_instrument(
        &conn_a,
        InstrumentInput {
            symbol: "BOND-1".into(),
            kind: InstrumentType::Bond,
            name: Some("国债 01".into()),
            currency_code: "CNY".into(),
            market: Some("unknown".into()),
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let row = |conn: &rusqlite::Connection| -> (String, String, Option<String>, String, String) {
        conn.query_row(
            "SELECT symbol, instrument_type, name, currency_code, market FROM instruments WHERE id=?1",
            [inst_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
    };
    assert_eq!(row(&conn_a), row(&conn_b), "标的建档重放后一致");

    // 幂等复用（同码同类型改名）→ Update op；无变化的复用不产出 op。
    let op_count_before = read_ops(&conn_a).unwrap().len();
    let same_id = create_instrument(
        &conn_a,
        InstrumentInput {
            symbol: "BOND-1".into(),
            kind: InstrumentType::Bond,
            name: Some("国债 01（改）".into()),
            currency_code: "CNY".into(),
            market: None,
        },
    )
    .unwrap();
    assert_eq!(same_id, inst_id, "同码同类型复用同一行");
    create_instrument(
        &conn_a,
        InstrumentInput {
            symbol: "BOND-1".into(),
            kind: InstrumentType::Bond,
            name: Some("国债 01（改）".into()),
            currency_code: "CNY".into(),
            market: None,
        },
    )
    .unwrap();
    assert_eq!(
        read_ops(&conn_a).unwrap().len(),
        op_count_before + 1,
        "改名复用产出 Update op"
    );
    create_instrument(
        &conn_a,
        InstrumentInput {
            symbol: "BOND-1".into(),
            kind: InstrumentType::Bond,
            name: Some("国债 01（改）".into()),
            currency_code: "CNY".into(),
            market: None,
        },
    )
    .unwrap();
    assert_eq!(
        read_ops(&conn_a).unwrap().len(),
        op_count_before + 1,
        "无变化复用不产出 op"
    );
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(row(&conn_a), row(&conn_b), "改名重放后一致");

    // 自建标的删除 → op；重放后两端皆无行。
    delete_instrument(&conn_a, &inst_id).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let exists = |conn: &rusqlite::Connection| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM instruments WHERE id=?1",
            [inst_id.as_str()],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(exists(&conn_a), 0);
    assert_eq!(exists(&conn_b), 0, "删除重放后同样消失");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn fund_add_by_code_syncs_dictionary_not_quote() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    // 按代码即拉（注入桩离线驱动）：标的字典随 op 同步；东财现价是行情外拉
    // 数据，不进 op——重放端字典有行、现价无行（各端自行拉行情）。
    let detail = crate::investment::FundDetail {
        code: "000001".into(),
        name: "华夏成长混合".into(),
        fund_class: "混合型-灵活".into(),
        nav: Some(crate::investment::FundNav {
            nav: 1.2345,
            nav_date: "2026-01-09".into(),
        }),
    };
    let mut fetch = |_: &str| Ok(detail.clone());
    let result = add_fund_by_code_with(&conn_a, "000001", &mut fetch).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    let row = |conn: &rusqlite::Connection| -> (String, String, Option<String>, String) {
        conn.query_row(
            "SELECT symbol, instrument_type, name, currency_code FROM instruments WHERE id=?1",
            [result.instrument_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    };
    assert_eq!(
        row(&conn_a),
        (
            "000001".into(),
            "fund".into(),
            Some("华夏成长混合".into()),
            "CNY".into()
        )
    );
    assert_eq!(row(&conn_a), row(&conn_b), "基金标的建档重放后一致");
    let quote_rows = |conn: &rusqlite::Connection| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM market_prices WHERE instrument_id=?1",
            [result.instrument_id.as_str()],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(quote_rows(&conn_a), 1, "源端落现价（东财净值）");
    assert_eq!(quote_rows(&conn_b), 0, "行情不随 op 同步（各端自拉）");
}

#[test]
fn exchange_rate_and_price_entries_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "inst-1");
    seed_investment_setup(&conn_b, "acc-inv", "inst-1");

    // 汇率录入 → op → 重放后汇率行一致（自然键 base/quote）。
    let rate_id = create_exchange_rate(
        &conn_a,
        crate::currencies::ExchangeRateInput {
            base_code: "EUR".into(),
            quote_code: "CNY".into(),
            rate: 7.8,
            priced_at: "2026-01-10".into(),
            source: Some("manual".into()),
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let rate_row = |conn: &rusqlite::Connection| -> (f64, String, String) {
        conn.query_row(
            "SELECT rate, priced_at, source FROM exchange_rates WHERE id=?1",
            [rate_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(
        rate_row(&conn_a),
        (7.8, "2026-01-10".into(), "manual".into())
    );
    assert_eq!(rate_row(&conn_a), rate_row(&conn_b), "汇率重放后一致");

    // 现价录入（独立写价通道）→ op → 重放后现价行一致。
    let _price_id = create_market_price(
        &conn_a,
        crate::investment::MarketPriceInput {
            instrument_id: "inst-1".into(),
            price_cents: 1525,
            currency_code: "USD".into(),
            priced_at: "2026-01-10".into(),
            source: None,
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    // 行 id 各端本地事实（upsert 未命中各自新建），按标的自然键读业务字段。
    let price_row = |conn: &rusqlite::Connection| -> (i64, String, String, Option<String>) {
        conn.query_row(
            "SELECT price_cents, priced_at, currency_code, nav_date FROM market_prices WHERE instrument_id='inst-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    };
    assert_eq!(
        price_row(&conn_a),
        (1525, "2026-01-10".into(), "USD".into(), None)
    );
    assert_eq!(price_row(&conn_a), price_row(&conn_b), "现价重放后一致");

    // 手动报价 → op → 两落点（周采样历史 + 现价映像）重放后一致。
    record_manual_price(
        &conn_a,
        &crate::investment::ManualPriceInput {
            instrument_id: "inst-1".into(),
            date: "2026-01-12".into(),
            price_cents: 1600,
        },
    )
    .unwrap();
    // 回填早于最新点的旧价：只沉淀历史、不改现价（最新点映像规则随 op 重放）。
    record_manual_price(
        &conn_a,
        &crate::investment::ManualPriceInput {
            instrument_id: "inst-1".into(),
            date: "2026-01-05".into(),
            price_cents: 1400,
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let history = |conn: &rusqlite::Connection| -> Vec<(String, i64, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT trade_date, price_cents, source FROM price_history \
                 WHERE instrument_id='inst-1' ORDER BY trade_date",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let current = |conn: &rusqlite::Connection| -> (i64, String) {
        conn.query_row(
            "SELECT price_cents, priced_at FROM market_prices WHERE instrument_id='inst-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(
        history(&conn_a),
        vec![
            ("2026-01-05".into(), 1400, "manual".into()),
            ("2026-01-12".into(), 1600, "manual".into()),
        ],
        "周采样历史两落点"
    );
    assert_eq!(
        current(&conn_a),
        (1600, "2026-01-12".into()),
        "回填旧价不动现价"
    );
    assert_eq!(history(&conn_a), history(&conn_b), "历史重放后一致");
    assert_eq!(current(&conn_a), current(&conn_b), "现价映像重放后一致");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn ai_batch_import_produces_ops_and_dedup_is_independent() {
    use crate::transaction::TransactionBatch;

    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_investment_setup(&conn_a, "acc-inv", "inst-1");
    seed_investment_setup(&conn_b, "acc-inv", "inst-1");

    // AI 批量导入（HTTP POST /transactions/batch 同一编排）：普通行 + buy 行，
    // 各带客户端幂等键（指向源文件行的稳定身份）。
    let mut expense = super::common::make_expense("acc-inv", 88_00, "外卖");
    expense.idempotency_key = Some("src.csv:1".into());
    let mut buy = buy_input("acc-inv", "inst-1", 100.0, 1500, 5);
    buy.idempotency_key = Some("src.csv:2".into());
    let outcome = TransactionBatch::run(&conn_a, vec![expense, buy], true).unwrap();
    assert_eq!(outcome.results.len(), 2);
    assert!(outcome.results.iter().all(|r| r.success && !r.duplicate));

    // 每条实际落库行各产一条 op（导入不绕过行为编排入口，op 产出同收敛）。
    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 2, "批量导入每实际落库行一条 op：{ops:?}");
    let buy_id = outcome.results[1].id.clone().unwrap();

    // 重跑同批（AI 纠错重试常态）：IdempotencyKey 命中 → 行被导入去重跳过，
    // 不产生第二笔交易，也不产生第二条 op——导入去重（幂等键）与 op 去重
    // （op_id）各自独立，互不冲突。
    let mut expense_again = super::common::make_expense("acc-inv", 88_00, "外卖");
    expense_again.idempotency_key = Some("src.csv:1".into());
    let mut buy_again = buy_input("acc-inv", "inst-1", 100.0, 1500, 5);
    buy_again.idempotency_key = Some("src.csv:2".into());
    let again = TransactionBatch::run(&conn_a, vec![expense_again, buy_again], true).unwrap();
    assert!(
        again.results.iter().all(|r| r.success && r.duplicate),
        "重跑同批全部按幂等键命中：{:?}",
        again.results
    );
    assert_eq!(read_ops(&conn_a).unwrap().len(), 2, "去重行不产出 op");
    let count: i64 = conn_a
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2, "重跑不产生重复交易");

    // 重放端收敛：batch 里的 buy 行重放后批次一致；重投递整批 op 幂等跳过
    // （op 去重按 op_id 生效，与导入去重无交涉）。
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(
        read_lot(&conn_a, &buy_id),
        read_lot(&conn_b, &buy_id),
        "批量导入的 buy 重放后批次一致"
    );
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Skipped));
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}
