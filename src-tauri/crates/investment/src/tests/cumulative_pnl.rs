//! 累计收益（CumulativePnl）按币种聚合测试（issue #1077 / 词汇表「累计收益」）。
//!
//! 领域绑定判据：累计收益 = 未实现盈亏（Holding，`v_holdings`）+ 已实现盈亏
//! （RealizedPnl，`security_lot_sales`）两腿相加；覆盖部分卖出、全清仓、
//! 基金转换（convert）、份额调整（split）四个场景，并在多币种账户下断言各币种
//! 独立成组、不跨币种求和。缺价 / 缺汇率持仓采 Holding 侧空值语义——未实现腿
//! 不计入且不以零计入（某币种两腿皆空则该组不出现）。

use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{
    create_transaction_internal, delete_transaction_internal, update_transaction_internal,
};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_exchange_rate, seed_instrument};

/// 种入标的当前行情（现价缓存单行，`v_holdings` 据此算市值与未实现盈亏）。
/// `market_prices` 不在种子工厂登记处，故按既有投资域测试先例裸插（形状见 trade 测试）。
fn seed_market_price(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    price_cents: i64,
    currency: &str,
) {
    let now = ledger_infra::db::now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9)",
        rusqlite::params![
            ledger_infra::db::new_uuid(),
            instrument_id,
            price_cents,
            currency,
            now,
            now,
            now,
            1,
            "test"
        ],
    )
    .unwrap();
}

/// 取指定币种的累计收益小计（不存在即 0）。
fn cumulative_for(groups: &[CurrencyCumulativePnl], currency: &str) -> i64 {
    groups
        .iter()
        .find(|g| g.currency_code == currency)
        .map(|g| g.cumulative_pnl_cents)
        .unwrap_or(0)
}

/// 既有两腿读口径求和（未实现自持仓视图、已实现自盈亏汇总）——绑定判据的对拍基准。
fn legs_of(conn: &rusqlite::Connection) -> (i64, i64) {
    let unrealized: i64 = list_holdings(conn)
        .unwrap()
        .iter()
        .filter_map(|h| h.unrealized_pnl_cents)
        .sum();
    let realized: i64 = query_realized_pnl_summary(
        conn,
        &PnlFilter {
            account_id: None,
            instrument_id: None,
        },
    )
    .unwrap()
    .total
    .iter()
    .map(|g| g.realized_pnl_cents)
    .sum();
    (unrealized, realized)
}

#[test]
fn cumulative_pnl_equals_unrealized_plus_realized_after_partial_sell() {
    let conn = open();
    seed_account(&conn, "acc-cp", "美股账户", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-cp", "AAPL", "Apple", "USD", "unknown");

    // 买 10 @100 元、卖 5 @120 元（费 2 元）→ 已实现 98 元 = 9800 分。
    create_transaction_internal(
        &conn,
        make_buy_input("acc-cp", "inst-cp", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cp", "inst-cp", 5.0, 1_200_000, 200),
    )
    .unwrap();
    // 剩余 5 份现价 110 元 → 市值 55000 分、成本 50000 分、未实现 5000 分。
    seed_market_price(&conn, "inst-cp", 1_100_000, "USD");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(realized, 9800);
    assert_eq!(unrealized, 5000);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(cumulative_for(&groups, "USD"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "USD"), 14800);
}

#[test]
fn cumulative_pnl_after_full_liquidation_equals_realized() {
    let conn = open();
    seed_account(&conn, "acc-cp", "美股账户", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-cp", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-cp", "inst-cp", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cp", "inst-cp", 10.0, 1_200_000, 0),
    )
    .unwrap();

    // 全清仓：无持仓行 → 未实现腿为空，累计收益 = 已实现 20000 分。
    assert!(list_holdings(&conn).unwrap().is_empty());
    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(unrealized, 0);
    assert_eq!(realized, 20000);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "USD"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "USD"), 20000);
}

#[test]
fn cumulative_pnl_groups_by_currency_without_mixing() {
    // 多币种账户（ADR-0107 决策 6 同款）：USD 与 CNY 各自成组，不出现跨币种求和。
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_account(&conn, "acc-cny", "A 股账户", "investment", "CNY", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-usd", "USDX", "USDX Corp", "USD", "unknown");
    seed_instrument(&conn, "inst-cny", "CNYX", "CNYX Corp", "CNY", "unknown");

    // USD：买 10 @100 元、卖 5 @120 元（费 2 元）、余 5 现价 110 元 → 未实现 5000、已实现 9800。
    create_transaction_internal(
        &conn,
        make_buy_input("acc-usd", "inst-usd", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-usd", "inst-usd", 5.0, 1_200_000, 200),
    )
    .unwrap();
    seed_market_price(&conn, "inst-usd", 1_100_000, "USD");

    // CNY：买 5 @10 元、卖 2 @6 元、余 3 现价 10 元 → 未实现 0、已实现 −8 元 = −800 分。
    create_transaction_internal(
        &conn,
        make_buy_input("acc-cny", "inst-cny", 5.0, 100_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cny", "inst-cny", 2.0, 60_000, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-cny", 100_000, "CNY");

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(cumulative_for(&groups, "USD"), 14800);
    assert_eq!(cumulative_for(&groups, "CNY"), -800);
    // 混算会得到 14000（把两币种裸数字相加），断言它不出现在任何分组。
    assert!(groups.iter().all(|g| g.cumulative_pnl_cents != 14000));
}

#[test]
fn cumulative_pnl_skips_holding_without_price() {
    // 缺价持仓采 Holding 侧空值语义：未实现腿为空 → 不计入、不以零计入。
    let conn = open();
    seed_account(&conn, "acc-nop", "美股账户", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-nop", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-nop", "inst-nop", 10.0, 1_000_000, 0),
    )
    .unwrap();

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert!(holdings[0].unrealized_pnl_cents.is_none());
    // 唯二腿皆空 → 不出现 USD 分组（而非 USD 0）。
    assert!(query_cumulative_pnl_summary(&conn).unwrap().is_empty());
}

#[test]
fn cumulative_pnl_skips_holding_without_fx_rate() {
    // 缺汇率与缺价同走 v_holdings 的空值分支：现价币种 ≠ 账户币种且两向汇率全缺时，
    // 市值 / 未实现盈亏为空 → 不计入、不以零计入。
    let conn = open();
    seed_account(&conn, "acc-fx", "美股账户", "investment", "USD", 0);
    // 建仓流水的本位币折算走 USD→CNY；与 JPY 报价腿无关。
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-fx", "7203", "Toyota", "JPY", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-fx", "inst-fx", 10.0, 1_000_000, 0),
    )
    .unwrap();
    // 现价以 JPY 报价：USD 账户缺 JPY↔USD 汇率 → v_holdings 市值/未实现为空。
    seed_market_price(&conn, "inst-fx", 1_100_000, "JPY");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert!(holdings[0].market_value_cents.is_none());
    assert!(holdings[0].unrealized_pnl_cents.is_none());
    assert!(query_cumulative_pnl_summary(&conn).unwrap().is_empty());
}

#[test]
fn cumulative_pnl_identity_holds_after_convert() {
    // 基金转换：转出腿按 FIFO 结转成本建入批次，零已实现盈亏；两腿相加恒等。
    let conn = open();
    seed_account(&conn, "acc-cv", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
    seed_instrument(&conn, "inst-in", "519700", "转入基金", "CNY", "unknown");

    // 买 10 @1 元；转换 4 份 A → 4 份 B（结转成本 400 分）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 4.0, 4.0, 400, 400, 0),
    )
    .unwrap();
    // 余 A 6 份现价 1 元（未实现 0）、B 4 份现价 1.2 元（未实现 80 分）。
    seed_market_price(&conn, "inst-out", 10_000, "CNY");
    seed_market_price(&conn, "inst-in", 12_000, "CNY");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(realized, 0);
    assert_eq!(unrealized, 80);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "CNY"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "CNY"), 80);
}

#[test]
fn cumulative_pnl_identity_holds_after_split() {
    // 份额调整：按比例重述批次成本，总成本不变；现价抬升即未实现盈亏，恒等成立。
    let conn = open();
    seed_account(&conn, "acc-sp", "股票户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sp", "502010", "证券基金", "CNY", "unknown");

    // 买 100 @1 元（成本 10000 分）；split +100 拆成 200 份，剩余成本仍 10000 分。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0)).unwrap();
    // 现价 1.2 元 → 市值 200 × 1.2 = 240 元 = 24000 分，未实现 14000 分。
    seed_market_price(&conn, "inst-sp", 12_000, "CNY");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(realized, 0);
    assert_eq!(unrealized, 14000);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "CNY"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "CNY"), 14000);
}

#[test]
fn cumulative_pnl_includes_dividend_as_third_leg() {
    // 分红计入累计收益的第三腿（issue #1078 / ADR-0109）：累计收益 = 未实现盈亏 +
    // 已实现盈亏 + 累计分红；分红不摊薄成本——未实现 / 已实现两腿逐位不变。
    let conn = open();
    seed_account(&conn, "acc-dv", "美股账户", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-dv", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-dv", "inst-dv", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-dv", "inst-dv", 5.0, 1_200_000, 200),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dv", 1_100_000, "USD");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(unrealized, 5000);
    assert_eq!(realized, 9800);
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "USD"),
        unrealized + realized
    );

    // 三笔分红共 30 元（3000 分）。
    for _ in 0..3 {
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 1000, "USD"))
            .unwrap();
    }

    // 两腿口径不变（分红不摊薄 FIFO 批次成本、不进已实现盈亏），第三腿 3000 分入累计。
    let (unrealized_after, realized_after) = legs_of(&conn);
    assert_eq!((unrealized_after, realized_after), (unrealized, realized));
    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "USD"), unrealized + realized + 3000);
    assert_eq!(cumulative_for(&groups, "USD"), 17800);
}

#[test]
fn cumulative_pnl_dividend_leg_survives_missing_price() {
    // 分红腿不受当前有无行情影响（空值语义只作用于未实现腿）：缺价持仓两腿为空，
    // 分红腿独立让该币种分组出现，且金额精确。
    let conn = open();
    seed_account(&conn, "acc-dvp", "美股账户", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-dvp", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-dvp", "inst-dvp", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dvp", "inst-dvp", 2500, "USD"),
    )
    .unwrap();

    // 无现价 → 未实现腿为空（不以零计入）；分组仍因分红腿出现。
    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(cumulative_for(&groups, "USD"), 2500);
}

#[test]
fn cumulative_pnl_dividend_without_holding_is_allowed() {
    // 分红不要求当前有持仓（除息 / 到账错位合法）：清仓后收到分红照常入第三腿。
    let conn = open();
    seed_account(&conn, "acc-dvc", "股票户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dvc", "502010", "证券基金", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-dvc",
            "inst-dvc",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-dvc",
            "inst-dvc",
            100.0,
            12_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    assert!(list_holdings(&conn).unwrap().is_empty());

    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dvc", "inst-dvc", 4000, "CNY"),
    )
    .unwrap();

    // 全清仓后的累计收益 = 已实现 2000（100 份 × (1.20 − 1.00) 元）+ 分红 4000
    //（无未实现腿）。
    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "CNY"), 6000);
}

#[test]
fn cumulative_pnl_dividend_update_and_delete_roll_back_precisely() {
    // 分红流水被修改 / 删除后，累计收益精确回退（实时重聚，无物化状态）。
    let conn = open();
    seed_account(&conn, "acc-dvr", "股票户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dvr", "502010", "证券基金", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-dvr",
            "inst-dvr",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dvr", 10_000, "CNY");

    let dividend_id = create_transaction_internal(
        &conn,
        make_dividend_input("acc-dvr", "inst-dvr", 3000, "CNY"),
    )
    .unwrap()
    .id;
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "CNY"),
        3000
    );

    // 全字段替换：金额 3000 → 5000，第三腿随之替换而非叠加。
    update_transaction_internal(
        &conn,
        &dividend_id,
        make_dividend_input("acc-dvr", "inst-dvr", 5000, "CNY"),
    )
    .unwrap();
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "CNY"),
        5000
    );

    // 删除：分红腿精确移除，只剩现价与成本相等的 0 未实现腿。
    delete_transaction_internal(&conn, &dividend_id).unwrap();
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "CNY"),
        0
    );
}

#[test]
fn cumulative_pnl_dividend_groups_by_currency_without_mixing() {
    // 分红腿与既有两腿同款多币种口径：按交易行币种分组，不跨币种裸数字相加。
    let conn = open();
    seed_account(&conn, "acc-dv-usd", "美股账户", "investment", "USD", 0);
    seed_account(&conn, "acc-dv-cny", "A 股账户", "investment", "CNY", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-dv-usd", "USDX", "USDX Corp", "USD", "unknown");
    seed_instrument(&conn, "inst-dv-cny", "CNYX", "CNYX Corp", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-usd", "inst-dv-usd", 1000, "USD"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-cny", "inst-dv-cny", 2000, "CNY"),
    )
    .unwrap();

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(cumulative_for(&groups, "USD"), 1000);
    assert_eq!(cumulative_for(&groups, "CNY"), 2000);
    // 混算会得到 3000（把两币种裸数字相加），断言它不出现在任何分组。
    assert!(groups.iter().all(|g| g.cumulative_pnl_cents != 3000));
}
